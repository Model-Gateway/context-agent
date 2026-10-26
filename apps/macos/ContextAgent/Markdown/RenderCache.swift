import Foundation

/// A bounded memo table over `NSCache` (evicts past `countLimit` and under
/// memory pressure). Transcript rows are rebuilt whenever their inputs
/// change or a chat is reopened; memoizing their parsed and rendered markdown
/// keeps that from re-parsing text and re-checking files on disk.
final class RenderCache<Key: Hashable, Value> {
    private final class WrappedKey: NSObject {
        let key: Key
        init(_ key: Key) { self.key = key }
        override var hash: Int { key.hashValue }
        override func isEqual(_ object: Any?) -> Bool { (object as? WrappedKey)?.key == key }
    }

    private final class Entry {
        let value: Value
        init(_ value: Value) { self.value = value }
    }

    private let cache = NSCache<WrappedKey, Entry>()

    init(countLimit: Int) {
        cache.countLimit = countLimit
    }

    func value(for key: Key, compute: () -> Value) -> Value {
        let wrapped = WrappedKey(key)
        if let entry = cache.object(forKey: wrapped) { return entry.value }
        let value = compute()
        cache.setObject(Entry(value), forKey: wrapped)
        return value
    }

    func removeAll() {
        cache.removeAllObjects()
    }
}
