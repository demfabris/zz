import Foundation

final class ZZSSHPromptBroker: @unchecked Sendable {
    private let condition = NSCondition()
    private let present: @Sendable (ZZSSHPromptRequest) -> Void
    private var nextID: UInt64 = 1
    private var activeID: UInt64?
    private var answer: ZZSSHPromptAnswer?
    private var cancelled = false
    private var initialSecret: String?

    init(
        initialSecret: String?,
        present: @escaping @Sendable (ZZSSHPromptRequest) -> Void
    ) {
        self.initialSecret = initialSecret
        self.present = present
    }

    func request(
        kind: ZZSSHPromptKind,
        title: String,
        message: String,
        echo: Bool
    ) -> ZZSSHPromptAnswer {
        condition.lock()
        guard !cancelled else {
            condition.unlock()
            return .cancel
        }
        let lowered = message.lowercased()
        if kind == .secret, let initialSecret,
           lowered.contains("password") || lowered.contains("passphrase") {
            self.initialSecret = nil
            condition.unlock()
            return .answer(initialSecret)
        }
        let id = nextID
        nextID &+= 1
        activeID = id
        answer = nil
        condition.unlock()

        present(Self.request(id: id, kind: kind, title: title, message: message, echo: echo))

        condition.lock()
        while !cancelled, activeID == id, answer == nil {
            condition.wait()
        }
        let result = activeID == id ? answer ?? .cancel : .cancel
        if activeID == id {
            activeID = nil
            answer = nil
        }
        condition.unlock()
        return result
    }

    func respond(to id: UInt64, with answer: ZZSSHPromptAnswer) {
        condition.lock()
        guard !cancelled, activeID == id else {
            condition.unlock()
            return
        }
        self.answer = answer
        condition.broadcast()
        condition.unlock()
    }

    func cancel() {
        condition.lock()
        cancelled = true
        answer = .cancel
        condition.broadcast()
        condition.unlock()
    }

    private static func request(
        id: UInt64,
        kind: ZZSSHPromptKind,
        title: String,
        message: String,
        echo: Bool
    ) -> ZZSSHPromptRequest {
        ZZSSHPromptRequest(id: id, kind: kind, title: title, message: message, echo: echo)
    }
}

let zzSSHPromptCallback: zz_ssh_prompt_callback = { context, prompt, response, capacity in
    guard let context, let prompt else {
        return ZZ_SSH_PROMPT_CANCEL
    }
    let broker = Unmanaged<ZZSSHPromptBroker>.fromOpaque(context).takeUnretainedValue()
    let value = prompt.pointee
    let kind = ZZSSHPromptKind(rawValue: UInt32(value.kind.rawValue)) ?? .secret
    let answer = broker.request(
        kind: kind,
        title: zzString(value.title),
        message: zzString(value.message),
        echo: value.echo
    )
    switch answer {
    case .cancel:
        return ZZ_SSH_PROMPT_CANCEL
    case .trustOnce where kind == .hostKey:
        return ZZ_SSH_PROMPT_TRUST_ONCE
    case .trustAndSave where kind == .hostKey:
        return ZZ_SSH_PROMPT_TRUST_AND_SAVE
    case let .answer(value):
        let bytes = Array(value.utf8)
        guard let response, capacity > bytes.count else {
            return ZZ_SSH_PROMPT_CANCEL
        }
        bytes.withUnsafeBufferPointer { input in
            response.withMemoryRebound(to: UInt8.self, capacity: capacity) { output in
                if let source = input.baseAddress {
                    output.update(from: source, count: input.count)
                }
                output[input.count] = 0
            }
        }
        return ZZ_SSH_PROMPT_ANSWER
    case .trustOnce, .trustAndSave:
        return ZZ_SSH_PROMPT_CANCEL
    }
}

private func zzString(_ bytes: zz_bytes) -> String {
    guard let pointer = bytes.ptr, bytes.len > 0 else {
        return ""
    }
    return String(
        decoding: UnsafeBufferPointer(start: pointer, count: bytes.len),
        as: UTF8.self
    )
}
