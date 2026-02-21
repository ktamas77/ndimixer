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

// MARK: - App Delegate

class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var timer: Timer?
    var statusURL: URL

    init(url: URL) {
        self.statusURL = url
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        setDisconnected()
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

    func pollStatus() {
        let task = URLSession.shared.dataTask(with: statusURL) { [weak self] data, _, error in
            DispatchQueue.main.async {
                guard let self = self else { return }
                if let data = data, error == nil,
                   let status = try? JSONDecoder().decode(StatusResponse.self, from: data)
                {
                    self.setConnected()
                    self.buildMenu(from: status)
                } else {
                    self.setDisconnected()
                    self.buildOfflineMenu()
                }
            }
        }
        task.resume()
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

    func buildMenu(from status: StatusResponse) {
        let menu = NSMenu()

        let header = NSMenuItem(
            title: "NDI Mixer v\(status.version) — \(formatUptime(status.uptime_seconds))",
            action: nil, keyEquivalent: "")
        header.isEnabled = false
        menu.addItem(header)
        menu.addItem(NSMenuItem.separator())

        for ch in status.channels {
            // Channel name header
            let chItem = NSMenuItem(title: "\u{25CF} \(ch.name)", action: nil, keyEquivalent: "")
            chItem.isEnabled = false
            let chAttrs: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 13, weight: .semibold),
            ]
            chItem.attributedTitle = NSAttributedString(string: "\u{25CF} \(ch.name)", attributes: chAttrs)
            menu.addItem(chItem)

            // NDI input
            if let ndi = ch.ndi_input {
                let symbol = ndi.connected ? "\u{2713}" : "\u{2717}"
                let ndiItem = NSMenuItem(
                    title: "  NDI: \(symbol) \(ndi.source)", action: nil, keyEquivalent: "")
                ndiItem.isEnabled = false
                menu.addItem(ndiItem)
            } else {
                let ndiItem = NSMenuItem(title: "  NDI: —", action: nil, keyEquivalent: "")
                ndiItem.isEnabled = false
                menu.addItem(ndiItem)
            }

            // Browser overlay
            if let browser = ch.browser_overlay {
                let symbol = browser.loaded ? "\u{2713}" : "\u{2717}"
                let bItem = NSMenuItem(
                    title: "  Browser: \(symbol) loaded", action: nil, keyEquivalent: "")
                bItem.isEnabled = false
                menu.addItem(bItem)
            } else {
                let bItem = NSMenuItem(title: "  Browser: —", action: nil, keyEquivalent: "")
                bItem.isEnabled = false
                menu.addItem(bItem)
            }

            // Output
            let outItem = NSMenuItem(
                title: "  Output: \(ch.output_name) \(ch.resolution)@\(ch.frame_rate)",
                action: nil, keyEquivalent: "")
            outItem.isEnabled = false
            menu.addItem(outItem)

            // Frames
            let framesItem = NSMenuItem(
                title: "  Frames: \(formatNumber(ch.frames_output))",
                action: nil, keyEquivalent: "")
            framesItem.isEnabled = false
            menu.addItem(framesItem)

            menu.addItem(NSMenuItem.separator())
        }

        let quitItem = NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q")
        quitItem.target = self
        menu.addItem(quitItem)

        statusItem.menu = menu
    }

    func buildOfflineMenu() {
        let menu = NSMenu()

        let offlineItem = NSMenuItem(
            title: "NDI Mixer: not running", action: nil, keyEquivalent: "")
        offlineItem.isEnabled = false
        menu.addItem(offlineItem)

        menu.addItem(NSMenuItem.separator())

        let quitItem = NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q")
        quitItem.target = self
        menu.addItem(quitItem)

        statusItem.menu = menu
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
app.setActivationPolicy(.accessory) // No dock icon
let delegate = AppDelegate(url: url)
app.delegate = delegate
app.run()
