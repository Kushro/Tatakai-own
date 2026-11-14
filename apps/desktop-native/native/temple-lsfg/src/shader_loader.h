// SPDX-License-Identifier: GPL-3.0-only
//
// Adapted from Enigmata's fg_shader_loader.h (see shader_loader.cpp for the provenance).
// Temple modification: in-memory shader sets, std::filesystem paths, no Android.
#pragma once

#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <vector>

namespace tlsfg {

// Result codes of the extraction.
namespace FgResult {
inline constexpr int32_t OK = 0;
inline constexpr int32_t DLL_UNREADABLE = -20;
inline constexpr int32_t MISSING_SPIRV = -21;
inline constexpr int32_t BINDING_REWRITE = -22;
}  // namespace FgResult

// Shader set resource ids inside Lossless.dll. A shader with base index b
// (0..kShaderCount-1) lives at kFp32FirstId + b and kFp16FirstId + b.
inline constexpr int32_t kShaderCount = 48;
inline constexpr int32_t kFp32FirstId = 353;
inline constexpr int32_t kFp32LastId = 400;
inline constexpr int32_t kFp16FirstId = 304;
inline constexpr int32_t kFp16LastId = 351;

// Same signature as the loader parameter of LSFG_3_1[P]::initialize.
using ShaderLoader = std::function<std::vector<uint8_t>(const std::string&)>;

// SPIR-V blobs already rewritten, indexed by base id. fp16 is empty unless the whole set was found.
struct ShaderSet {
    std::vector<std::vector<uint8_t>> fp32;
    std::vector<std::vector<uint8_t>> fp16;
    bool hasFp16() const { return fp16.size() == static_cast<size_t>(kShaderCount); }
};

// Reads the DLL and fills `out`. Returns FgResult::OK, DLL_UNREADABLE, MISSING_SPIRV or BINDING_REWRITE.
int32_t extractShaders(const std::filesystem::path& dllPath, ShaderSet& out);

// Loader over an extracted set; uses FP16 only when useFp16 and the FP16 set is complete.
ShaderLoader makeLoader(std::shared_ptr<const ShaderSet> set, bool useFp16);

// Maps a framegen shader name (e.g. "p_mipmaps", "alpha[2]", "generate") to its base index, or -1.
int32_t nameToBaseId(const std::string& name);

}  // namespace tlsfg
