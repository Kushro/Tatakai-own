// SPDX-License-Identifier: AGPL-3.0-or-later
package android.os;

/**
 * 0.4.0 (sub-translation worker): JVM stand-in with non-null strings. Media3's ParsableByteArray reads {@code Build.FINGERPRINT} in a
 * static initializer and crashes on the mockable android.jar's null. {@code Build.VERSION} / {@code VERSION_CODES} still come from
 * android.jar (separate classes).
 */
public class Build {
    public static final String UNKNOWN = "unknown";
    public static final String BOARD = UNKNOWN;
    public static final String BOOTLOADER = UNKNOWN;
    public static final String BRAND = UNKNOWN;
    public static final String DEVICE = UNKNOWN;
    public static final String DISPLAY = UNKNOWN;
    public static final String FINGERPRINT = "jvm-unit-test";
    public static final String HARDWARE = UNKNOWN;
    public static final String HOST = UNKNOWN;
    public static final String ID = UNKNOWN;
    public static final String MANUFACTURER = UNKNOWN;
    public static final String MODEL = UNKNOWN;
    public static final String PRODUCT = UNKNOWN;
    public static final String RADIO = UNKNOWN;
    public static final String SERIAL = UNKNOWN;
    public static final String SOC_MANUFACTURER = UNKNOWN;
    public static final String SOC_MODEL = UNKNOWN;
    public static final String TAGS = UNKNOWN;
    public static final String TYPE = UNKNOWN;
    public static final String USER = UNKNOWN;
    public static final long TIME = 0L;
    public static final String[] SUPPORTED_ABIS = new String[0];
    public static final String[] SUPPORTED_32_BIT_ABIS = new String[0];
    public static final String[] SUPPORTED_64_BIT_ABIS = new String[0];
}
