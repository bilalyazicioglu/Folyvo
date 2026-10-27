//! Folyvo's four actions under right-click › Services. Finder extensions never run in iCloud Drive
//! folders (spike S11), Services do. Each entry asks the app to open a window, like a Finder intent.

use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeFileURL, NSUpdateDynamicServices};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString, NSURL};
use tauri::AppHandle;

use crate::intent::{act_on, Intent, IntentKind};

pub struct Ivars {
    app: AppHandle,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and ServiceProvider does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FolyvoServiceProvider"]
    #[ivars = Ivars]
    struct ServiceProvider;

    unsafe impl NSObjectProtocol for ServiceProvider {}

    // The selectors are `<NSMessage>:userData:error:` from Info.plist.
    impl ServiceProvider {
        #[unsafe(method(browse:userData:error:))]
        fn browse(&self, pboard: &NSPasteboard, _user: Option<&NSString>, _error: *mut *mut NSString) {
            self.handle("browse", pboard);
        }

        #[unsafe(method(send:userData:error:))]
        fn send(&self, pboard: &NSPasteboard, _user: Option<&NSString>, _error: *mut *mut NSString) {
            self.handle("send", pboard);
        }

        #[unsafe(method(saveCopy:userData:error:))]
        fn save_copy(&self, pboard: &NSPasteboard, _user: Option<&NSString>, _error: *mut *mut NSString) {
            self.handle("saveCopy", pboard);
        }

        #[unsafe(method(whichOne:userData:error:))]
        fn which_one(&self, pboard: &NSPasteboard, _user: Option<&NSString>, _error: *mut *mut NSString) {
            self.handle("whichOne", pboard);
        }
    }
);

impl ServiceProvider {
    fn handle(&self, message: &str, pboard: &NSPasteboard) {
        let (Some(kind), Some(path)) = (IntentKind::from_service_message(message), first_file(pboard)) else {
            return;
        };
        act_on(&self.ivars().app, Intent { kind, path });
    }
}

/// The first file on the pasteboard Services handed over.
fn first_file(pboard: &NSPasteboard) -> Option<PathBuf> {
    let item = pboard.pasteboardItems()?.firstObject()?;
    // SAFETY: AppKit's own constant type name.
    let url = item.stringForType(unsafe { NSPasteboardTypeFileURL })?;
    let path = NSURL::URLWithString(&url)?.path()?;
    Some(PathBuf::from(path.to_string()))
}

thread_local! {
    // NSApplication keeps its services provider weakly.
    static PROVIDER: std::cell::RefCell<Option<Retained<ServiceProvider>>> = const { std::cell::RefCell::new(None) };
}

/// Makes Folyvo answer its Services entries. Call on the main thread once the app is set up.
pub fn register(app: &AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let provider = ServiceProvider::alloc(mtm).set_ivars(Ivars { app: app.clone() });
    // SAFETY: NSObject's init on a freshly allocated object.
    let provider: Retained<ServiceProvider> = unsafe { msg_send![super(provider), init] };
    // SAFETY: the provider answers the selectors Info.plist names and lives as long as the app.
    unsafe { NSApplication::sharedApplication(mtm).setServicesProvider(Some(&provider)) };
    NSUpdateDynamicServices();
    PROVIDER.with(|p| *p.borrow_mut() = Some(provider));
}
