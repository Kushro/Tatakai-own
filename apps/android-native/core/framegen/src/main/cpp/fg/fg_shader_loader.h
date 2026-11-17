// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): extraction of the precompiled SPIR-V shaders
// from the user's Lossless.dll and the loader handed to LSFG initialize.
// Layout on disk: <dir>/fp32/<id>.spv (required set) and <dir>/fp16/<id>.spv
// (optional set). No DXBC path. The DLL is read into memory and parsed with
// pe-parse's ParsePEFromPointer (no mmap, no fd kept open).
#pragma once

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace efg {

// Shader set resource ids inside Lossless.dll. A shader with base index b
// (0..kShaderCount-1) lives at kFp32FirstId + b and kFp16FirstId + b.
inline constexpr int32_t kShaderCount = 48;
inline constexpr int32_t kFp32FirstId = 353;
inline constexpr int32_t kFp32LastId = 400;
inline constexpr int32_t kFp16FirstId = 304;
inline constexpr int32_t kFp16LastId = 351;

// Same signature as the loader parameter of LSFG_3_1[P]::initialize. Returns
// an empty vector for an unknown or missing shader.
using ShaderLoader = std::function<std::vector<uint8_t>(const std::string&)>;

// Extracts the FP32 set (all required) and the FP16 set (best effort), rewrites
// the bindings and writes the .spv files under outDir. Returns the number of
// FP32 shaders written (>= 0) or FgResult::DLL_UNREADABLE, MISSING_SPIRV,
// BINDING_REWRITE or WRITE_FAILED.
int32_t extractShaders(const std::string& dllPath, const std::string& outDir);

// True when dir/fp32 holds the complete FP32 set.
bool probeShaders(const std::string& dir);

// Loader reading from dir; uses fp16 only when allowFp16 and the FP16 set is
// complete, otherwise fp32.
ShaderLoader makeLoader(const std::string& dir, bool allowFp16);

// Maps a framegen shader name (e.g. "p_mipmaps", "alpha[2]", "generate") to
// its base index 0..kShaderCount-1, or -1 if unknown.
int32_t nameToBaseId(const std::string& name);

}  // namespace efg
