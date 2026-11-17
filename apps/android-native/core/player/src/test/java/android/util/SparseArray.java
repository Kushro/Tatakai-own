// SPDX-License-Identifier: AGPL-3.0-or-later
package android.util;

import java.util.TreeMap;

/**
 * 0.4.0 (sub-translation worker): JVM stand-in for the framework class, so Media3's MatroskaExtractor runs in plain unit tests
 * (the mockable android.jar returns 0 / null from every method). Test classes come first on the unit-test classpath.
 */
public class SparseArray<E> {
    private final TreeMap<Integer, E> map = new TreeMap<>();

    public SparseArray() {}

    public SparseArray(int initialCapacity) {}

    public E get(int key) { return map.get(key); }

    public E get(int key, E valueIfKeyNotFound) { return map.getOrDefault(key, valueIfKeyNotFound); }

    public void put(int key, E value) { map.put(key, value); }

    public void append(int key, E value) { map.put(key, value); }

    public void remove(int key) { map.remove(key); }

    public void delete(int key) { map.remove(key); }

    public int size() { return map.size(); }

    public int keyAt(int index) { return (Integer) map.keySet().toArray()[index]; }

    @SuppressWarnings("unchecked")
    public E valueAt(int index) { return (E) map.values().toArray()[index]; }

    public int indexOfKey(int key) {
        int i = 0;
        for (Integer k : map.keySet()) {
            if (k == key) return i;
            i++;
        }
        return -1;
    }

    public boolean contains(int key) { return map.containsKey(key); }

    public void clear() { map.clear(); }
}
