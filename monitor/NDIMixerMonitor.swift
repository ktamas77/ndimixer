import AppKit
import Foundation

// MARK: - JSON Models

struct StatusResponse: Codable {
    let version: String
    let compositor: String?
    let uptime_seconds: UInt64
    let channels: [ChannelStatus]
}

struct ChannelStatus: Codable {
    let name: String
    let output_name: String
    let resolution: String
    let frame_rate: UInt32
    let ndi_input: NdiInputStatus?
    let browser_overlays: [BrowserOverlayStatus]
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
    var browserItems: [NSMenuItem]
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
    var isOnline = false

    // Track menu structure: overlay count per channel
    var lastMenuSignature: [Int] = []

    // FPS tracking: previous frame counts and poll timestamp
    var lastFrameCounts: [UInt64] = []
    var lastPollTime: Date?

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
        let t = Timer(timeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.pollStatus()
        }
        // Add to .common modes so the timer fires while the menu is open
        RunLoop.main.add(t, forMode: .common)
        timer = t
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
        lastMenuSignature = []
    }

    func menuSignature(for channels: [ChannelStatus]) -> [Int] {
        channels.map { $0.browser_overlays.count }
    }

    func rebuildChannelItems(channels: [ChannelStatus]) {
        // Remove old channel items
        for items in channelItems {
            menu.removeItem(items.nameItem)
            menu.removeItem(items.ndiItem)
            for bi in items.browserItems {
                menu.removeItem(bi)
            }
            menu.removeItem(items.outputItem)
            menu.removeItem(items.framesItem)
        }
        // Remove old separators between channels
        while let sepIdx = findChannelSeparatorIndex() {
            menu.removeItem(at: sepIdx)
        }

        channelItems = []
        var insertIdx = menu.index(of: quitItem)

        for ch in channels {
            let browserCount = max(ch.browser_overlays.count, 1) // at least 1 line for "—"

            let nameItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            nameItem.isEnabled = false
            menu.insertItem(nameItem, at: insertIdx)
            insertIdx += 1

            let ndiItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            ndiItem.isEnabled = false
            menu.insertItem(ndiItem, at: insertIdx)
            insertIdx += 1

            var browserItems: [NSMenuItem] = []
            for _ in 0..<browserCount {
                let bi = NSMenuItem(title: "", action: nil, keyEquivalent: "")
                bi.isEnabled = false
                menu.insertItem(bi, at: insertIdx)
                insertIdx += 1
                browserItems.append(bi)
            }

            let outputItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            outputItem.isEnabled = false
            menu.insertItem(outputItem, at: insertIdx)
            insertIdx += 1

            let framesItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            framesItem.isEnabled = false
            menu.insertItem(framesItem, at: insertIdx)
            insertIdx += 1

            let sep = NSMenuItem.separator()
            sep.tag = 999
            menu.insertItem(sep, at: insertIdx)
            insertIdx += 1

            channelItems.append(ChannelMenuItems(
                nameItem: nameItem,
                ndiItem: ndiItem,
                browserItems: browserItems,
                outputItem: outputItem,
                framesItem: framesItem
            ))
        }

        lastMenuSignature = menuSignature(for: channels)
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
        let comp = status.compositor?.uppercased() ?? "CPU"
        headerItem.title = "NDI Mixer v\(status.version) \(comp) — \(formatUptime(status.uptime_seconds))"
        headerItem.isHidden = false
        offlineItem.isHidden = true

        // Rebuild channel structure if layout changed
        let sig = menuSignature(for: status.channels)
        if sig != lastMenuSignature {
            rebuildChannelItems(channels: status.channels)
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

            if ch.browser_overlays.isEmpty {
                if let first = items.browserItems.first {
                    first.title = "  Browser: \u{2014}"
                }
            } else {
                for (j, overlay) in ch.browser_overlays.enumerated() {
                    guard j < items.browserItems.count else { break }
                    let symbol = overlay.loaded ? "\u{2713}" : "\u{2717}"
                    items.browserItems[j].title = "  Browser: \(symbol) \(truncateURL(overlay.url))"
                }
            }

            items.outputItem.title = "  Output: \(ch.output_name) \(ch.resolution)@\(ch.frame_rate)"

            // Calculate FPS from frame count delta
            var fpsStr = ""
            if let prevTime = lastPollTime, i < lastFrameCounts.count {
                let dt = Date().timeIntervalSince(prevTime)
                if dt > 0.1 {
                    let delta = ch.frames_output >= lastFrameCounts[i]
                        ? ch.frames_output - lastFrameCounts[i] : 0
                    let fps = Double(delta) / dt
                    fpsStr = " (\(String(format: "%.1f", fps)) fps)"
                }
            }
            items.framesItem.title = "  Frames: \(formatNumber(ch.frames_output))\(fpsStr)"
        }

        // Store frame counts and timestamp for next FPS calculation
        lastFrameCounts = status.channels.map { $0.frames_output }
        lastPollTime = Date()
    }

    func showOffline() {
        isOnline = false
        headerItem.isHidden = true
        offlineItem.isHidden = false

        // Clear channel items
        if !lastMenuSignature.isEmpty {
            rebuildChannelItems(channels: [])
        }
    }

    func truncateURL(_ urlString: String, maxLength: Int = 40) -> String {
        var s = urlString
        if s.hasPrefix("https://") { s = String(s.dropFirst(8)) }
        else if s.hasPrefix("http://") { s = String(s.dropFirst(7)) }
        if s.count > maxLength {
            return String(s.prefix(maxLength - 1)) + "\u{2026}"
        }
        return s
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
