//! Send a Copy: the macOS share sheet for a read-only copy of a version. The sheet only says that a
//! service took the file (spike S12), so a share is recorded as "attached in Mail", never "sent".

/// The titles the system gives its own services in the person's language.
#[derive(Debug, Default, Clone)]
pub struct KnownTitles {
    pub mail: String,
    pub airdrop: String,
    pub messages: String,
    /// The picker's Copy entry has no public name; its title in the languages Folyvo knows.
    pub copy: Vec<String>,
}

const OTHER_MAX: usize = 58;

/// The `service` recorded for a share: `mail`, `airdrop`, `messages`, `copy`, or `other:<title>`.
pub fn service_key(title: &str, known: &KnownTitles) -> String {
    let is = |t: &str| !t.is_empty() && t == title;
    if is(&known.mail) {
        "mail".into()
    } else if is(&known.airdrop) {
        "airdrop".into()
    } else if is(&known.messages) {
        "messages".into()
    } else if known.copy.iter().any(|c| is(c)) {
        "copy".into()
    } else {
        let clean: String = title.chars().filter(|c| !c.is_control()).take(OTHER_MAX).collect();
        format!("other:{}", if clean.trim().is_empty() { "App" } else { clean.as_str() })
    }
}

/// What AppKit holds only weakly while a share is under way, kept here until that share ends. A new
/// share never drops one still running: an AirDrop reports `didShareItems` only when it finishes.
pub struct Holding<T> {
    next: u64,
    held: Vec<(u64, T)>,
}

impl<T> Default for Holding<T> {
    fn default() -> Self {
        Holding { next: 0, held: Vec::new() }
    }
}

impl<T> Holding<T> {
    /// Keeps `value` and returns the number that lets it go.
    pub fn hold(&mut self, value: T) -> u64 {
        self.next += 1;
        self.held.push((self.next, value));
        self.next
    }

    /// Hands back what `id` kept, once.
    pub fn take(&mut self, id: u64) -> Option<T> {
        let at = self.held.iter().position(|(i, _)| *i == id)?;
        Some(self.held.swap_remove(at).1)
    }

    pub fn release(&mut self, id: u64) {
        self.take(id);
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// The macOS share sheet, shown from a view of a Folyvo window.
#[cfg(target_os = "macos")]
pub mod mac {
    use std::cell::RefCell;
    use std::path::Path;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
    use objc2_app_kit::{
        NSSharingService, NSSharingServiceDelegate, NSSharingServiceNameComposeEmail,
        NSSharingServiceNameComposeMessage, NSSharingServiceNameSendViaAirDrop, NSSharingServicePicker,
        NSSharingServicePickerDelegate, NSView,
    };
    use objc2_foundation::{
        NSArray, NSError, NSObject, NSObjectProtocol, NSRect, NSRectEdge, NSString, NSURL,
    };

    use super::{service_key, KnownTitles};

    /// Called once with the service's title when a service took the file.
    type OnShare = Box<dyn FnOnce(String)>;

    pub struct Ivars {
        on_share: RefCell<Option<OnShare>>,
        /// Where this share's picker and delegate are kept in `SHOWN`.
        held: std::cell::Cell<u64>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and ShareDelegate does not implement Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FolyvoShareDelegate"]
        #[ivars = Ivars]
        struct ShareDelegate;

        unsafe impl NSObjectProtocol for ShareDelegate {}

        unsafe impl NSSharingServicePickerDelegate for ShareDelegate {
            #[unsafe(method_id(sharingServicePicker:delegateForSharingService:))]
            fn delegate_for(
                &self,
                _picker: &NSSharingServicePicker,
                _service: &NSSharingService,
            ) -> Option<Retained<ProtocolObject<dyn NSSharingServiceDelegate>>> {
                Some(ProtocolObject::from_retained(self.retain()))
            }

            // No service: the sheet was dismissed and nothing was shared.
            #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
            fn did_choose(&self, _picker: &NSSharingServicePicker, service: Option<&NSSharingService>) {
                if service.is_none() {
                    self.finish();
                }
            }
        }

        unsafe impl NSSharingServiceDelegate for ShareDelegate {
            // The service took the file. For Mail this is before Send is pressed (spike S12).
            #[unsafe(method(sharingService:didShareItems:))]
            fn did_share(&self, service: &NSSharingService, _items: &NSArray) {
                if let Some(on_share) = self.ivars().on_share.borrow_mut().take() {
                    on_share(service.title().to_string());
                }
                self.finish();
            }

            #[unsafe(method(sharingService:didFailToShareItems:error:))]
            fn did_fail(&self, _service: &NSSharingService, _items: &NSArray, _error: &NSError) {
                self.ivars().on_share.borrow_mut().take();
                self.finish();
            }
        }
    );

    impl ShareDelegate {
        /// Lets this share's picker and delegate go once AppKit is done with the current event: they
        /// may be in the middle of calling us, so they go to the autorelease pool, not away at once.
        fn finish(&self) {
            if let Some((picker, delegate)) = SHOWN.with(|s| s.borrow_mut().take(self.ivars().held.get())) {
                let _ = Retained::autorelease_ptr(picker);
                let _ = Retained::autorelease_ptr(delegate);
            }
        }
    }

    thread_local! {
        // AppKit holds a picker and its delegate weakly; each pair stays here until its share ends.
        static SHOWN: RefCell<super::Holding<(Retained<NSSharingServicePicker>, Retained<ShareDelegate>)>> =
            RefCell::new(super::Holding::default());
    }

    /// The system services' titles in the person's language.
    pub fn known_titles() -> KnownTitles {
        let title = |name| {
            NSSharingService::sharingServiceNamed(name).map(|s| s.title().to_string()).unwrap_or_default()
        };
        KnownTitles {
            // SAFETY: AppKit's own constant names, read once.
            mail: title(unsafe { NSSharingServiceNameComposeEmail }),
            airdrop: title(unsafe { NSSharingServiceNameSendViaAirDrop }),
            messages: title(unsafe { NSSharingServiceNameComposeMessage }),
            copy: vec!["Copy".into(), "Kopyala".into()],
        }
    }

    /// Shows the share sheet for `file` next to `rect` in `view`; `on_record` gets the service key once.
    pub fn show(
        mtm: MainThreadMarker,
        view: &NSView,
        rect: NSRect,
        file: &Path,
        on_record: impl FnOnce(String) + 'static,
    ) {
        let known = known_titles();
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
        let items = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(url))]);
        let delegate = ShareDelegate::alloc(mtm).set_ivars(Ivars {
            on_share: RefCell::new(Some(Box::new(move |title: String| {
                on_record(service_key(&title, &known))
            }))),
            held: std::cell::Cell::new(0),
        });
        // SAFETY: NSObject's init on a freshly allocated object.
        let delegate: Retained<ShareDelegate> = unsafe { msg_send![super(delegate), init] };
        // SAFETY: the items are file URLs, which conform to NSPasteboardWriting.
        let picker =
            unsafe { NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items) };
        picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let id = SHOWN.with(|s| s.borrow_mut().hold((picker.clone(), delegate.clone())));
        delegate.ivars().held.set(id);
        picker.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_share_does_not_drop_one_still_under_way() {
        let mut held = Holding::default();
        let airdrop = held.hold("airdrop");
        let mail = held.hold("mail");
        held.release(airdrop);
        assert_eq!(held.len(), 1, "the mail share is still held");
        held.release(airdrop);
        held.release(mail);
        assert_eq!(held.len(), 0);
    }

    #[test]
    fn system_services_get_their_short_names() {
        let known = KnownTitles {
            mail: "Mail".into(),
            airdrop: "AirDrop".into(),
            messages: "Mesajlar".into(),
            copy: vec!["Copy".into(), "Kopyala".into()],
        };
        assert_eq!(service_key("Mail", &known), "mail");
        assert_eq!(service_key("AirDrop", &known), "airdrop");
        assert_eq!(service_key("Mesajlar", &known), "messages");
        assert_eq!(service_key("Kopyala", &known), "copy");
        assert_eq!(service_key("Blip", &known), "other:Blip");
    }

    #[test]
    fn an_unknown_title_is_kept_short_and_printable() {
        let known = KnownTitles::default();
        let key = service_key(&format!("A\u{7}{}", "ğ".repeat(80)), &known);
        assert!(folyvo_core::handed::validate_service(&key).is_ok(), "{key}");
        assert_eq!(key.chars().count(), "other:".len() + 58);
        assert!(
            folyvo_core::handed::validate_service(&service_key("\u{7}", &known)).is_ok(),
            "an empty title still records"
        );
    }
}
