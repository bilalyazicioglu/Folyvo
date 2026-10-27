// Folyvo's items in Finder's right-click menu. Stateless: it watches the home folder, shows a fixed menu,
// and asks the app to open a window with a folyvo:// URL. It never changes anything itself.
import Cocoa
import FinderSync

/// Menu titles and the intent each one sends. Finder hands back a copy of the menu item, without its
/// representedObject, so the title is how a choice is recognised.
let items: [(title: String, kind: String)] = [
    ("Browse All Versions…", "browse"),
    ("Send a Copy…", "send"),
    ("Save a Copy…", "save_copy"),
    ("Which One Is This?…", "which_one"),
]

/// The person's home folder. Inside the sandbox, FileManager answers with the extension's own container.
func realHome() -> String {
    if let entry = getpwuid(getuid()), let dir = entry.pointee.pw_dir { return String(cString: dir) }
    return NSHomeDirectory()
}

@objc(FinderSync)
final class FinderSync: FIFinderSync {
    override init() {
        super.init()
        // Only the home folder: the app follows a link only for a file there, so items shown
        // anywhere else would do nothing. Services cover other disks.
        FIFinderSyncController.default().directoryURLs = [URL(fileURLWithPath: realHome(), isDirectory: true)]
    }

    override func menu(for kind: FIMenuKind) -> NSMenu {
        let menu = NSMenu(title: "")
        guard kind == .contextualMenuForItems else { return menu }
        for item in items {
            menu.addItem(NSMenuItem(title: item.title, action: #selector(choose(_:)), keyEquivalent: ""))
        }
        return menu
    }

    @objc func choose(_ sender: NSMenuItem) {
        guard let kind = items.first(where: { $0.title == sender.title })?.kind,
              let file = FIFinderSyncController.default().selectedItemURLs()?.first
        else { return }
        // URLComponents would leave & = + in a name as they are; the app reads those as separators.
        var allowed = CharacterSet.urlQueryAllowed
        allowed.remove(charactersIn: "&=+")
        guard let path = file.path.addingPercentEncoding(withAllowedCharacters: allowed) else { return }
        let link = URL(string: "folyvo://intent?kind=\(kind)&path=\(path)")
        if let link { NSWorkspace.shared.open(link) }
    }
}
