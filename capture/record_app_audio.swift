// Record only an explicitly supplied, owned FistWorld client/capture PID.
// Build from the repository root (the executable and recordings stay ignored):
//   swiftc -O -parse-as-library "$PWD/capture/record_app_audio.swift" -o logs/tools/record_app_audio
// Usage: record_app_audio PID OUTPUT.wav STOP_FLAG [READY_FLAG] [TIMEOUT_SECONDS]
// Create STOP_FLAG after the semantic rehearsal finishes. READY_FLAG defaults to
// OUTPUT.wav.ready. This tool never requests capture or microphone permission.

import AppKit
import AVFoundation
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

struct RecordingFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
    init(_ message: String) { self.message = message }
}

func writeJSON(_ value: [String: Any], to url: URL) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: url, options: .atomic)
}

final class AppAudioOutput: NSObject, SCStreamOutput, SCStreamDelegate {
    let queue = DispatchQueue(label: "fistworld.capture.application-audio")
    private let url: URL
    private var file: AVAudioFile?
    private var format: AVAudioFormat?
    private var failure: String?
    private var totalFrames: Int64 = 0
    private var nonzeroFrames: Int64 = 0
    private var squared = [Double](repeating: 0, count: 2)
    private var peaks = [Float](repeating: 0, count: 2)
    private var firstPTS: Double?
    private var lastPTS: Double?

    init(url: URL) { self.url = url }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        queue.async { self.failure = error.localizedDescription }
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer,
                of type: SCStreamOutputType) {
        guard type == .audio else { return }
        appendAudio(sample)
    }

    // Called on queue; kept separate so the WAV/data-layout path can be checked
    // using synthetic PCM buffers without opening any capture stream.
    func appendAudio(_ sample: CMSampleBuffer) {
        guard CMSampleBufferDataIsReady(sample), failure == nil else { return }
        do {
            guard let description = CMSampleBufferGetFormatDescription(sample) else {
                throw RecordingFailure("Audio sample has no format description.")
            }
            let inputFormat = AVAudioFormat(cmAudioFormatDescription: description)
            guard inputFormat.sampleRate == 48_000, inputFormat.channelCount == 2,
                  inputFormat.commonFormat == .pcmFormatFloat32 else {
                throw RecordingFailure("Expected 48 kHz stereo float audio, received \(inputFormat).")
            }
            let frames = CMSampleBufferGetNumSamples(sample)
            guard frames > 0, frames <= Int(Int32.max),
                  let buffer = AVAudioPCMBuffer(pcmFormat: inputFormat,
                                                frameCapacity: AVAudioFrameCount(frames)) else {
                throw RecordingFailure("Invalid captured audio frame count.")
            }
            buffer.frameLength = AVAudioFrameCount(frames)
            let status = CMSampleBufferCopyPCMDataIntoAudioBufferList(
                sample, at: 0, frameCount: Int32(frames), into: buffer.mutableAudioBufferList)
            guard status == noErr else {
                throw RecordingFailure("Could not copy captured PCM samples (status \(status)).")
            }
            if file == nil {
                let settings: [String: Any] = [
                    AVFormatIDKey: kAudioFormatLinearPCM,
                    AVSampleRateKey: 48_000,
                    AVNumberOfChannelsKey: 2,
                    AVLinearPCMBitDepthKey: 32,
                    AVLinearPCMIsFloatKey: true,
                    AVLinearPCMIsBigEndianKey: false,
                    AVLinearPCMIsNonInterleaved: false,
                ]
                file = try AVAudioFile(forWriting: url, settings: settings,
                                       commonFormat: inputFormat.commonFormat,
                                       interleaved: inputFormat.isInterleaved)
                format = inputFormat
            }
            guard format == inputFormat, let channels = buffer.floatChannelData else {
                throw RecordingFailure("Capture audio format changed during the recording.")
            }
            // Keep floating-point evidence: a clipped live mix must not be
            // silently clamped by a PCM16 export before we inspect it.
            for frame in 0..<frames {
                var audible = false
                for channel in 0..<2 {
                    let value = channels[channel][frame * buffer.stride]
                    guard value.isFinite else {
                        throw RecordingFailure("Nonfinite sample in captured application audio.")
                    }
                    peaks[channel] = max(peaks[channel], abs(value))
                    squared[channel] += Double(value) * Double(value)
                    audible = audible || abs(value) > 0.000001
                }
                if audible { nonzeroFrames += 1 }
            }
            try file?.write(from: buffer)
            totalFrames += Int64(frames)
            let pts = CMSampleBufferGetPresentationTimeStamp(sample).seconds
            if firstPTS == nil { firstPTS = pts }
            lastPTS = pts + Double(frames) / 48_000
        } catch {
            failure = error.localizedDescription
        }
    }

    func currentFailure() -> String? { queue.sync { failure } }

    func finish() -> [String: Any] {
        queue.sync {
            file = nil // Finalize the WAV header before reporting completion.
            return [
                "frames": totalFrames,
                "nonzero_frames": nonzeroFrames,
                "duration_seconds": Double(totalFrames) / 48_000,
                "sample_rate": 48_000,
                "channels": 2,
                "codec": "pcm_f32le",
                "peak_linear_by_channel": peaks,
                "rms_linear_by_channel": squared.map { sqrt($0 / Double(max(totalFrames, 1))) },
                "first_sample_pts_seconds": firstPTS as Any? ?? NSNull(),
                "last_sample_end_pts_seconds": lastPTS as Any? ?? NSNull(),
                "error": failure as Any? ?? NSNull(),
                "listening_approved": false,
            ]
        }
    }
}

@main
struct RecordAppAudio {
    static func main() async {
        do { try await run() }
        catch {
            FileHandle.standardError.write(Data("Application audio capture failed: \(error.localizedDescription)\n".utf8))
            exit(1)
        }
    }

    static func run() async throws {
        let args = Array(CommandLine.arguments.dropFirst())
        guard (3...5).contains(args.count), let pid = Int32(args[0]), pid > 0 else {
            throw RecordingFailure("Usage: record_app_audio PID OUTPUT.wav STOP_FLAG [READY_FLAG] [TIMEOUT_SECONDS]")
        }
        let output = URL(fileURLWithPath: args[1]).standardizedFileURL
        let stop = URL(fileURLWithPath: args[2]).standardizedFileURL
        let ready = args.count > 3 ? URL(fileURLWithPath: args[3]).standardizedFileURL
                                  : output.appendingPathExtension("ready")
        let timeout = args.count > 4 ? Double(args[4]) : 120
        guard let timeout, timeout.isFinite, timeout > 0, timeout <= 120 else {
            throw RecordingFailure("Hard timeout must be greater than zero and at most 120 seconds.")
        }
        let reportURL = output.appendingPathExtension("capture-audio.json")
        guard output.pathExtension.lowercased() == "wav", Set([output, stop, ready, reportURL]).count == 4 else {
            throw RecordingFailure("Output must be a WAV, with distinct output, stop, ready and report paths.")
        }
        for path in [output, stop, ready, reportURL] where FileManager.default.fileExists(atPath: path.path) {
            throw RecordingFailure("Refusing an existing output/control file: \(path.path)")
        }
        // Read-only check. Never call CGRequestScreenCaptureAccess or a picker.
        guard CGPreflightScreenCaptureAccess() else {
            throw RecordingFailure("Existing screen/audio capture access is unavailable; no permission was requested.")
        }
        guard let process = NSRunningApplication(processIdentifier: pid),
              let executable = process.executableURL?.resolvingSymlinksInPath(),
              ["client", "capture"].contains(executable.lastPathComponent) else {
            throw RecordingFailure("Target PID is not an owned FistWorld client/capture application.")
        }
        let repository = URL(fileURLWithPath: #filePath).standardizedFileURL
            .deletingLastPathComponent().deletingLastPathComponent()
        guard executable.path.hasPrefix(repository.path + "/target/") else {
            throw RecordingFailure("Target executable must be under this repository's target directory.")
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        guard let application = content.applications.first(where: { $0.processID == pid }),
              let display = content.displays.first(where: { $0.displayID == CGMainDisplayID() }) ?? content.displays.first else {
            throw RecordingFailure("Target PID is not currently available as a shareable application.")
        }
        let filter = SCContentFilter(display: display, including: [application], exceptingWindows: [])
        let configuration = SCStreamConfiguration()
        configuration.capturesAudio = true
        configuration.captureMicrophone = false
        configuration.excludesCurrentProcessAudio = true
        configuration.sampleRate = 48_000
        configuration.channelCount = 2
        // No screen output is attached. Keep the unused video configuration tiny.
        configuration.width = 2
        configuration.height = 2
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        configuration.queueDepth = 3
        for directory in Set([output.deletingLastPathComponent(), ready.deletingLastPathComponent()]) {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        let sink = AppAudioOutput(url: output)
        let stream = SCStream(filter: filter, configuration: configuration, delegate: sink)
        try stream.addStreamOutput(sink, type: .audio, sampleHandlerQueue: sink.queue)
        try await stream.startCapture()
        let began = Date()
        var reason = "stop_flag"
        do {
            try writeJSON(["pid": pid, "application": application.applicationName,
                           "started_at": ISO8601DateFormatter().string(from: began),
                           "sample_rate": 48_000, "channels": 2, "microphone": false,
                           "output": output.path], to: ready)
            print("READY \(ready.path)")
            fflush(stdout)
            while !FileManager.default.fileExists(atPath: stop.path) {
                if let failure = sink.currentFailure() { throw RecordingFailure(failure) }
                if process.isTerminated { reason = "target_exited"; break }
                if Date().timeIntervalSince(began) >= timeout { reason = "hard_timeout"; break }
                try await Task.sleep(nanoseconds: 100_000_000)
            }
        } catch {
            reason = "capture_error: \(error.localizedDescription)"
        }
        do { try await stream.stopCapture() }
        catch { reason = "stop_error: \(error.localizedDescription)" }
        var report = sink.finish()
        report["pid"] = pid
        report["application"] = application.applicationName
        report["executable"] = executable.path
        report["output"] = output.path
        report["microphone"] = false
        report["application_filter_only"] = true
        report["stop_reason"] = reason
        report["wall_duration_seconds"] = Date().timeIntervalSince(began)
        try writeJSON(report, to: reportURL)
        print("COMPLETE \(reportURL.path)")
        guard reason == "stop_flag" || reason == "target_exited", sink.currentFailure() == nil,
              let frames = report["frames"] as? Int64, frames > 0 else {
            throw RecordingFailure("Capture did not complete with audio frames (\(reason)); inspect \(reportURL.path).")
        }
    }
}
