// SPDX-License-Identifier: MIT
//
// Temple: the one symbol pe-parse needs from its unicode layer, declared in <pe-parse/to_string.h>
// (POSIX version; unicode_win.cpp is the Windows one). pe-parse decodes the UTF-16LE names of the PE
// resource directory with it. Outside MSVC UCharString is std::u16string, so the conversion to UTF-8
// is done by hand (no ICU, no deprecated <codecvt>).
#include <pe-parse/to_string.h>

#include <cstddef>
#include <string>

namespace peparse {

namespace {

void appendUtf8(std::string& out, char32_t cp) {
    if (cp < 0x80) {
        out += static_cast<char>(cp);
    } else if (cp < 0x800) {
        out += static_cast<char>(0xC0 | (cp >> 6));
        out += static_cast<char>(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        out += static_cast<char>(0xE0 | (cp >> 12));
        out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
        out += static_cast<char>(0x80 | (cp & 0x3F));
    } else {
        out += static_cast<char>(0xF0 | (cp >> 18));
        out += static_cast<char>(0x80 | ((cp >> 12) & 0x3F));
        out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
        out += static_cast<char>(0x80 | (cp & 0x3F));
    }
}

}  // namespace

std::string from_utf16(const UCharString &u) {
    std::string out;
    out.reserve(u.size());
    constexpr char32_t kReplacement = 0xFFFD;
    for (std::size_t i = 0; i < u.size(); ++i) {
        const char32_t c = static_cast<char16_t>(u[i]);
        if (c >= 0xD800 && c <= 0xDBFF) {
            // High surrogate: needs a low surrogate right after it.
            if (i + 1 < u.size()) {
                const char32_t d = static_cast<char16_t>(u[i + 1]);
                if (d >= 0xDC00 && d <= 0xDFFF) {
                    appendUtf8(out, 0x10000 + ((c - 0xD800) << 10) + (d - 0xDC00));
                    ++i;
                    continue;
                }
            }
            appendUtf8(out, kReplacement);
        } else if (c >= 0xDC00 && c <= 0xDFFF) {
            appendUtf8(out, kReplacement);  // lone low surrogate
        } else {
            appendUtf8(out, c);
        }
    }
    return out;
}

}  // namespace peparse
