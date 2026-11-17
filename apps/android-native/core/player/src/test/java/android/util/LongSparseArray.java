// SPDX-License-Identifier: AGPL-3.0-or-later
package android.util;

import java.util.TreeMap;

/** 0.4.0 (sub-translation worker): JVM stand-in for Media3's extractors in unit tests (see SparseArray). */
public class LongSparseArray<E> {
    private final TreeMap<Long, E> map = new TreeMap<>();

    public LongSparseArray() {}

    public LongSparseArray(int initialCapacity) {}

    public E get(long key) { return map.get(key); }

    public void put(long key, E value) { map.put(key, value); }

    public void append(long key, E value) { map.put(key, value); }

    public void remove(long key) { map.remove(key); }

    public int size() { return map.size(); }

    public long keyAt(int index) { return (Long) map.keySet().toArray()[index]; }

    @SuppressWarnings("unchecked")
    public E valueAt(int index) { return (E) map.values().toArray()[index]; }

    public void clear() { map.clear(); }
}
