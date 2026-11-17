// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/unicode_minimal.cpp, GPL-3.0.
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Replaces ICU for pe-parse. pe-parse only needs one symbol from its unicode
// layer, declared in <pe-parse/to_string.h>:
//     std::string peparse::from_utf16(const UCharString& u);
// (UCharString is std::u16string outside MSVC). It is used to decode PE
// resource directory names, which are UTF-16LE.
//
// Conversion rules: well-formed surrogate pairs become one 4-byte UTF-8
// sequence; an unpaired surrogate is replaced with U+FFFD (the reference
// encoded it raw, which yields invalid UTF-8).

#include <pe-parse/to_string.h>

#include <cstddef>
#include <cstdint>
#include <string>

namespace peparse {

namespace {

constexpr std::uint32_t kReplacement = 0xFFFDu;

void appendUtf8(std::string& out, std::uint32_t cp) {
    if (cp < 0x80u) {
        out.push_back(static_cast<char>(cp));
    } else if (cp < 0x800u) {
        out.push_back(static_cast<char>(0xC0u | (cp >> 6)));
        out.push_back(static_cast<char>(0x80u | (cp & 0x3Fu)));
    } else if (cp < 0x10000u) {
        out.push_back(static_cast<char>(0xE0u | (cp >> 12)));
        out.push_back(static_cast<char>(0x80u | ((cp >> 6) & 0x3Fu)));
        out.push_back(static_cast<char>(0x80u | (cp & 0x3Fu)));
    } else {
        out.push_back(static_cast<char>(0xF0u | (cp >> 18)));
        out.push_back(static_cast<char>(0x80u | ((cp >> 12) & 0x3Fu)));
        out.push_back(static_cast<char>(0x80u | ((cp >> 6) & 0x3Fu)));
        out.push_back(static_cast<char>(0x80u | (cp & 0x3Fu)));
    }
}

}  // namespace

std::string from_utf16(const UCharString& u) {
    std::string out;
    out.reserve(u.size());
    const std::size_t n = u.size();
    for (std::size_t i = 0; i < n; ++i) {
        std::uint32_t cp = static_cast<std::uint16_t>(u[i]);
        if (cp >= 0xD800u && cp <= 0xDBFFu) {
            // High surrogate: needs a following low surrogate.
            if (i + 1 < n) {
                const std::uint32_t low = static_cast<std::uint16_t>(u[i + 1]);
                if (low >= 0xDC00u && low <= 0xDFFFu) {
                    cp = 0x10000u + ((cp - 0xD800u) << 10) + (low - 0xDC00u);
                    ++i;
                } else {
                    cp = kReplacement;
                }
            } else {
                cp = kReplacement;
            }
        } else if (cp >= 0xDC00u && cp <= 0xDFFFu) {
            // Lone low surrogate.
            cp = kReplacement;
        }
        appendUtf8(out, cp);
    }
    return out;
}

}  // namespace peparse
