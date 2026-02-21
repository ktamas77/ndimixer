import AppKit
import Foundation

// MARK: - JSON Models

struct StatusResponse: Codable {
    let version: String
    let uptime_seconds: UInt64
    let channels: [ChannelStatus]
}

struct ChannelStatus: Codable {
    let name: String
    let output_name: String
    let resolution: String
    let frame_rate: UInt32
    let ndi_input: NdiInputStatus?
    let browser_overlay: BrowserOverlayStatus?
    let frames_output: UInt64
}

struct NdiInputStatus: Codable {
    let source: String
    let connected: Bool
    let frames_received: UInt64
}

struct BrowserOverlayStatus: Codable {
    let url: String
    let loaded: Bool
}

// MARK: - Per-channel menu item references for live updates

struct ChannelMenuItems {
    let nameItem: NSMenuItem
    let ndiItem: NSMenuItem
    let browserItem: NSMenuItem
    let outputItem: NSMenuItem
    let framesItem: NSMenuItem
}

// MARK: - App Delegate

class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var timer: Timer?
    var statusURL: URL

    // Persistent menu + item references for in-place updates
    var menu = NSMenu()
    var headerItem: NSMenuItem!
    var offlineItem: NSMenuItem!
    var channelItems: [ChannelMenuItems] = []
    var quitItem: NSMenuItem!
    var lastChannelCount: Int = -1
    var isOnline = false

    init(url: URL) {
        self.statusURL = url
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.menu = menu
        setDisconnected()
        buildInitialMenu()
        pollStatus()
        timer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.pollStatus()
        }
    }

    func setConnected() {
        let attrs: [NSAttributedString.Key: Any] = [
            .foregroundColor: NSColor.systemGreen,
            .font: NSFont.monospacedSystemFont(ofSize: 12, weight: .bold),
        ]
        statusItem.button?.attributedTitle = NSAttributedString(string: "NDI", attributes: attrs)
    }

    func setDisconnected() {
        let attrs: [NSAttributedString.Key: Any] = [
            .foregroundColor: NSColor.systemGray,
            .font: NSFont.monospacedSystemFont(ofSize: 12, weight: .bold),
        ]
        statusItem.button?.attributedTitle = NSAttributedString(string: "NDI", attributes: attrs)
    }

    func buildInitialMenu() {
        menu.removeAllItems()

        headerItem = NSMenuItem(title: "NDI Mixer: connecting...", action: nil, keyEquivalent: "")
        headerItem.isEnabled = false
        menu.addItem(headerItem)

        offlineItem = NSMenuItem(title: "NDI Mixer: not running", action: nil, keyEquivalent: "")
        offlineItem.isEnabled = false
        offlineItem.isHidden = true
        menu.addItem(offlineItem)

        menu.addItem(NSMenuItem.separator())

        quitItem = NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q")
        quitItem.target = self
        menu.addItem(quitItem)

        channelItems = []
        lastChannelCount = -1
    }

    func rebuildChannelItems(count: Int) {
        // Remove old channel items (between separator and quit)
        for items in channelItems {
            menu.removeItem(items.nameItem)
            menu.removeItem(items.ndiItem)
            menu.removeItem(items.browserItem)
            menu.removeItem(items.outputItem)
            menu.removeItem(items.framesItem)
        }
        // Remove old separators between channels
        while let sepIdx = findChannelSeparatorIndex() {
            menu.removeItem(at: sepIdx)
        }

        channelItems = []
        let insertIdx = menu.index(of: quitItem)

        for i in 0..<count {
            let offset = insertIdx + i * 6 // 5 items + 1 separator per channel

            let nameItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            nameItem.isEnabled = false
            menu.insertItem(nameItem, at: offset)

            let ndiItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            ndiItem.isEnabled = false
            menu.insertItem(ndiItem, at: offset + 1)

            let browserItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            browserItem.isEnabled = false
            menu.insertItem(browserItem, at: offset + 2)

            let outputItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            outputItem.isEnabled = false
            menu.insertItem(outputItem, at: offset + 3)

            let framesItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            framesItem.isEnabled = false
            menu.insertItem(framesItem, at: offset + 4)

            let sep = NSMenuItem.separator()
            sep.tag = 999 // tag to identify channel separators
            menu.insertItem(sep, at: offset + 5)

            channelItems.append(ChannelMenuItems(
                nameItem: nameItem,
                ndiItem: ndiItem,
                browserItem: browserItem,
                outputItem: outputItem,
                framesItem: framesItem
            ))
        }

        lastChannelCount = count
    }

    func findChannelSeparatorIndex() -> Int? {
        for i in 0..<menu.items.count {
            if menu.items[i].tag == 999 {
                return i
            }
        }
        return nil
    }

    func pollStatus() {
        let task = URLSession.shared.dataTask(with: statusURL) { [weak self] data, _, error in
            DispatchQueue.main.async {
                guard let self = self else { return }
                if let data = data, error == nil,
                   let status = try? JSONDecoder().decode(StatusResponse.self, from: data)
                {
                    self.setConnected()
                    self.updateMenu(from: status)
                } else {
                    self.setDisconnected()
                    self.showOffline()
                }
            }
        }
        task.resume()
    }

    func updateMenu(from status: StatusResponse) {
        isOnline = true
        headerItem.title = "NDI Mixer v\(status.version) — \(formatUptime(status.uptime_seconds))"
        headerItem.isHidden = false
        offlineItem.isHidden = true

        // Rebuild channel structure if count changed
        if status.channels.count != lastChannelCount {
            rebuildChannelItems(count: status.channels.count)
        }

        // Update each channel's items in place
        for (i, ch) in status.channels.enumerated() {
            guard i < channelItems.count else { break }
            let items = channelItems[i]

            let boldAttrs: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 13, weight: .semibold),
            ]
            items.nameItem.attributedTitle = NSAttributedString(
                string: "\u{25CF} \(ch.name)", attributes: boldAttrs)

            if let ndi = ch.ndi_input {
                let symbol = ndi.connected ? "\u{2713}" : "\u{2717}"
                items.ndiItem.title = "  NDI: \(symbol) \(ndi.source)"
            } else {
                items.ndiItem.title = "  NDI: \u{2014}"
            }

            if let browser = ch.browser_overlay {
                let symbol = browser.loaded ? "\u{2713}" : "\u{2717}"
                items.browserItem.title = "  Browser: \(symbol) loaded"
            } else {
                items.browserItem.title = "  Browser: \u{2014}"
            }

            items.outputItem.title = "  Output: \(ch.output_name) \(ch.resolution)@\(ch.frame_rate)"
            items.framesItem.title = "  Frames: \(formatNumber(ch.frames_output))"
        }
    }

    func showOffline() {
        isOnline = false
        headerItem.isHidden = true
        offlineItem.isHidden = false

        // Clear channel items
        if lastChannelCount != 0 {
            rebuildChannelItems(count: 0)
        }
    }

    func formatUptime(_ seconds: UInt64) -> String {
        let h = seconds / 3600
        let m = (seconds % 3600) / 60
        if h > 0 {
            return "up \(h)h \(m)m"
        } else {
            return "up \(m)m"
        }
    }

    func formatNumber(_ n: UInt64) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        return formatter.string(from: NSNumber(value: n)) ?? "\(n)"
    }

    @objc func quit() {
        NSApplication.shared.terminate(nil)
    }
}

// MARK: - Main

var urlString = "http://localhost:9100/status"

// Parse --url argument
let args = CommandLine.arguments
if let idx = args.firstIndex(of: "--url"), idx + 1 < args.count {
    urlString = args[idx + 1]
}

guard let url = URL(string: urlString) else {
    fputs("Error: invalid URL '\(urlString)'\n", stderr)
    exit(1)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = AppDelegate(url: url)
app.delegate = delegate
app.run()
