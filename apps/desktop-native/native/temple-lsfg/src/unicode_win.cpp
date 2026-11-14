// SPDX-License-Identifier: MIT
//
// Temple: the one symbol pe-parse needs from its unicode layer, declared in <pe-parse/to_string.h>.
// pe-parse decodes the UTF-16LE names of the PE resource directory with it. Under MSVC
// UCharString is std::wstring, so Win32 does the conversion.
#include <pe-parse/to_string.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <string>

namespace peparse {

std::string from_utf16(const UCharString &u) {
    if (u.empty()) return {};
    const int needed = WideCharToMultiByte(CP_UTF8, 0, u.data(), static_cast<int>(u.size()), nullptr, 0, nullptr, nullptr);
    if (needed <= 0) return {};
    std::string out(static_cast<size_t>(needed), '\0');
    WideCharToMultiByte(CP_UTF8, 0, u.data(), static_cast<int>(u.size()), out.data(), needed, nullptr, nullptr);
    return out;
}

}  // namespace peparse
