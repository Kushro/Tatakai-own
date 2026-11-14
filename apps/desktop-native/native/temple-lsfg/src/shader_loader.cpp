// SPDX-License-Identifier: GPL-3.0-only
//
// Adapted from the shader loader of Enigmata (same author), itself adapted from
// FrankBarretta/LSFG-Android-Application (commit b84754199823615d32d65fe33ea59481dca88dcf),
// app/src/main/cpp/android_shader_loader.cpp, GPL-3.0.
// Temple modification: Windows host build. Shaders are kept in memory (no cache on disk, no
// POSIX file API, no Android logging); the extraction, the SPIR-V version cap, the dense
// binding rewrite and the framegen name table are the same as in Enigmata. Combined work
// under GPL-3.0 (see README.md of native/temple-lsfg).
//
// Extraction of the precompiled SPIR-V shader sets from the user's Lossless.dll:
//   FP32: RT_RCDATA ids 353..400 (all 48 required).
//   FP16: RT_RCDATA ids 304..351 (optional and all-or-nothing).
// A shader with base index b (0..47) lives at 353 + b (FP32) and 304 + b (FP16).
// The DLL is read into memory and parsed with ParsePEFromPointer.

#include "shader_loader.h"

#include <pe-parse/parse.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <limits>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace tlsfg {

namespace {

static_assert(kFp32LastId - kFp32FirstId + 1 == kShaderCount, "FP32 range must hold kShaderCount ids");
static_assert(kFp16LastId - kFp16FirstId + 1 == kShaderCount, "FP16 range must hold kShaderCount ids");

constexpr uint32_t kSpirvMagicWord = 0x07230203u;
// SPIR-V LE magic as bytes.
constexpr std::array<uint8_t, 4> kSpirvMagic{0x03, 0x02, 0x23, 0x07};
// SPIR-V header: [magic][version][generator][bound][schema].
constexpr size_t kSpirvHeaderBytes = 20;

using BlobMap = std::unordered_map<uint32_t, std::vector<uint8_t>>;

struct ExtractionCtx {
    BlobMap* out;
};

bool inRange(uint32_t id, int32_t first, int32_t last) {
    return id >= static_cast<uint32_t>(first) && id <= static_cast<uint32_t>(last);
}

// IterRsrc callback (reference on_resource): keeps only RT_RCDATA blobs whose
// numeric name falls inside one of the two SPIR-V ranges. The bytes are copied
// out because res.buf points into the DLL vector.
int onResource(void* userData, const peparse::resource& res) {
    auto* ctx = static_cast<ExtractionCtx*>(userData);
    if (res.type != peparse::RT_RCDATA || res.buf == nullptr || res.buf->buf == nullptr ||
        res.buf->bufLen == 0) {
        return 0;
    }
    if (!inRange(res.name, kFp32FirstId, kFp32LastId) &&
        !inRange(res.name, kFp16FirstId, kFp16LastId)) {
        return 0;
    }
    std::vector<uint8_t> data(res.buf->bufLen);
    std::copy_n(res.buf->buf, res.buf->bufLen, data.data());
    (*ctx->out)[res.name] = std::move(data);
    return 0;
}

bool hasSpirvMagic(const std::vector<uint8_t>& blob) {
    return blob.size() >= kSpirvHeaderBytes &&
           std::equal(kSpirvMagic.begin(), kSpirvMagic.end(), blob.begin());
}

// Caps the SPIR-V version word at 1.3 (0x00010300). Vulkan 1.1-class drivers
// (Mali among them) reject newer module versions at pipeline creation.
void capSpirvVersion(std::vector<uint8_t>& spirv) {
    if (spirv.size() < kSpirvHeaderBytes) return;
    uint32_t ver;
    std::memcpy(&ver, spirv.data() + 4, 4);
    if (ver > 0x00010300u) {
        ver = 0x00010300u;
        std::memcpy(spirv.data() + 4, &ver, 4);
    }
}

// Renumbers the Binding decorations to the dense layout framegen builds.
//
// Framegen's ShaderModule creates its VkDescriptorSetLayout by counting
// bindings from 0 over a type-grouped list: uniform buffers, then samplers,
// then sampled images, then storage images (e.g. v3.1p_src/shaders/mipmaps.cpp
// declares { 1, UNIFORM_BUFFER }, { 1, SAMPLER }, { 1, SAMPLED_IMAGE },
// { 7, STORAGE_IMAGE }). The precompiled SPIR-V in Lossless.dll carries HLSL
// register slots instead (sampler, sampled image, storage images..., constant
// buffer last), which misroutes descriptors (black flashes, a growing black
// rectangle). Each variable is classified by the kind of its pointee type and
// given a dense binding in that group order, ties broken by the original
// binding so in-group order (Output0..Output6) is preserved.
//
// Returns false on a malformed module or an unclassifiable binding; the caller
// must then reject the blob rather than cache a misrouted shader.
bool rewriteSpirvBindingsDense(std::vector<uint8_t>& spirv) {
    if (spirv.size() < kSpirvHeaderBytes || (spirv.size() % 4) != 0) {
        return false;
    }
    const size_t wordCount = spirv.size() / 4;
    std::vector<uint32_t> words(wordCount);
    std::memcpy(words.data(), spirv.data(), spirv.size());
    if (words[0] != kSpirvMagicWord) {
        return false;
    }

    constexpr uint32_t kOpTypeImage = 25;
    constexpr uint32_t kOpTypeSampler = 26;
    constexpr uint32_t kOpTypeSampledImage = 27;
    constexpr uint32_t kOpTypeStruct = 30;
    constexpr uint32_t kOpTypePointer = 32;
    constexpr uint32_t kOpVariable = 59;
    constexpr uint32_t kOpDecorate = 71;
    constexpr uint32_t kOpFunction = 54;
    constexpr uint32_t kDecorationBinding = 33;

    // Lower value = earlier in framegen's layout.
    enum Kind : int {
        KindUniformBuffer = 0,
        KindSampler = 1,
        KindSampledImage = 2,
        KindStorageImage = 3,
        KindUnknown = 4,
    };

    struct BindingSite {
        size_t valueWordIndex;
        uint32_t varId;
        uint32_t origBinding;
        Kind kind;
    };

    std::unordered_map<uint32_t, Kind> typeKind;        // type id -> kind
    std::unordered_map<uint32_t, uint32_t> ptrPointee;  // pointer type id -> pointee type id
    std::unordered_map<uint32_t, uint32_t> varType;     // variable id -> pointer type id
    std::vector<BindingSite> sites;
    sites.reserve(32);

    // Declarations only: bindings never appear after the first OpFunction.
    size_t i = 5;  // skip the 5-word header
    while (i < wordCount) {
        const uint32_t header = words[i];
        const uint32_t wc = (header >> 16) & 0xFFFFu;
        const uint32_t op = header & 0xFFFFu;
        if (wc == 0 || i + wc > wordCount) {
            return false;  // malformed
        }
        if (op == kOpFunction) {
            break;
        }
        switch (op) {
            case kOpTypeSampler:
                if (wc >= 2) typeKind[words[i + 1]] = KindSampler;
                break;
            case kOpTypeImage:
                // id, sampledType, dim, depth, arrayed, ms, sampled, format;
                // sampled == 2 means storage image.
                if (wc >= 8) {
                    typeKind[words[i + 1]] = (words[i + 7] == 2) ? KindStorageImage : KindSampledImage;
                }
                break;
            case kOpTypeSampledImage:
                if (wc >= 2) typeKind[words[i + 1]] = KindSampledImage;
                break;
            case kOpTypeStruct:
                // Any struct backing a binding is the constant buffer (layout
                // verified by the reference across all FP16/FP32 variants).
                if (wc >= 2) typeKind[words[i + 1]] = KindUniformBuffer;
                break;
            case kOpTypePointer:
                // id, storageClass, type
                if (wc == 4) ptrPointee[words[i + 1]] = words[i + 3];
                break;
            case kOpVariable:
                // resultType (pointer), resultId, storageClass, [initializer]
                if (wc >= 4) varType[words[i + 2]] = words[i + 1];
                break;
            case kOpDecorate:
                if (wc == 4 && words[i + 2] == kDecorationBinding) {
                    sites.push_back({i + 3, words[i + 1], words[i + 3], KindUnknown});
                }
                break;
            default:
                break;
        }
        i += wc;
    }

    for (auto& s : sites) {
        const auto vt = varType.find(s.varId);
        if (vt == varType.end()) continue;
        const auto pp = ptrPointee.find(vt->second);
        if (pp == ptrPointee.end()) continue;
        const auto tk = typeKind.find(pp->second);
        if (tk != typeKind.end()) s.kind = tk->second;
    }
    for (const auto& s : sites) {
        if (s.kind == KindUnknown) return false;
    }
    std::stable_sort(sites.begin(), sites.end(), [](const BindingSite& a, const BindingSite& b) {
        if (a.kind != b.kind) return static_cast<int>(a.kind) < static_cast<int>(b.kind);
        return a.origBinding < b.origBinding;
    });
    for (size_t k = 0; k < sites.size(); ++k) {
        words[sites[k].valueWordIndex] = static_cast<uint32_t>(k);
    }
    std::memcpy(spirv.data(), words.data(), spirv.size());
    return true;
}


bool readWholeFile(const std::filesystem::path& path, std::vector<uint8_t>& out) {
    std::ifstream f(path, std::ios::binary | std::ios::ate);
    if (!f) return false;
    const std::streamoff size = f.tellg();
    if (size < 0) return false;
    out.resize(static_cast<size_t>(size));
    f.seekg(0, std::ios::beg);
    if (size > 0 && !f.read(reinterpret_cast<char*>(out.data()), static_cast<std::streamsize>(size))) {
        out.clear();
        return false;
    }
    return true;
}

// Validates and rewrites the 48 blobs of one set (base index order). Returns
// FgResult::OK, MISSING_SPIRV or BINDING_REWRITE; `firstBad` gets the id.
int32_t prepareSet(BlobMap& blobs, int32_t firstId, std::vector<std::vector<uint8_t>>& out,
                   int32_t& firstBad) {
    out.clear();
    out.reserve(kShaderCount);
    for (int32_t b = 0; b < kShaderCount; ++b) {
        const int32_t id = firstId + b;
        const auto it = blobs.find(static_cast<uint32_t>(id));
        if (it == blobs.end() || !hasSpirvMagic(it->second)) {
            firstBad = id;
            return FgResult::MISSING_SPIRV;
        }
        std::vector<uint8_t> blob = std::move(it->second);
        blobs.erase(it);
        if (!rewriteSpirvBindingsDense(blob)) {
            firstBad = id;
            return FgResult::BINDING_REWRITE;
        }
        capSpirvVersion(blob);
        out.push_back(std::move(blob));
    }
    return FgResult::OK;
}


}  // namespace

int32_t extractShaders(const std::filesystem::path& dllPath, ShaderSet& out) {
    out = ShaderSet{};
    std::vector<uint8_t> dll;
    if (!readWholeFile(dllPath, dll) || dll.empty() ||
        dll.size() > static_cast<size_t>(std::numeric_limits<uint32_t>::max())) {
        return FgResult::DLL_UNREADABLE;
    }

    BlobMap blobs;
    {
        peparse::parsed_pe* pe =
            peparse::ParsePEFromPointer(dll.data(), static_cast<uint32_t>(dll.size()));
        if (pe == nullptr) {
            return FgResult::DLL_UNREADABLE;
        }
        ExtractionCtx ctx{&blobs};
        try {
            peparse::IterRsrc(pe, onResource, &ctx);
        } catch (...) {
            peparse::DestructParsedPE(pe);
            throw;
        }
        peparse::DestructParsedPE(pe);
    }
    std::vector<uint8_t>().swap(dll);

    int32_t bad = 0;
    const int32_t rc32 = prepareSet(blobs, kFp32FirstId, out.fp32, bad);
    if (rc32 != FgResult::OK) {
        out = ShaderSet{};
        return rc32;
    }
    int32_t bad16 = 0;
    if (prepareSet(blobs, kFp16FirstId, out.fp16, bad16) != FgResult::OK) {
        out.fp16.clear();
    }
    return FgResult::OK;
}

ShaderLoader makeLoader(std::shared_ptr<const ShaderSet> set, bool useFp16) {
    const bool fp16 = useFp16 && set->hasFp16();
    return [set, fp16](const std::string& name) -> std::vector<uint8_t> {
        const int32_t b = nameToBaseId(name);
        if (b < 0) return {};
        const auto& list = fp16 ? set->fp16 : set->fp32;
        if (static_cast<size_t>(b) >= list.size()) return {};
        return list[static_cast<size_t>(b)];
    };
}

// Reference table (Extract::nameIdxTable) rebased to base indices 0..47; see
// the id scheme at the top of this file. Covers every literal framegen passes
// to getShader/getPipeline in v3.1_src and v3.1p_src (52 names; "mipmaps" and
// "p_mipmaps" share index 0, "generate" and "p_generate" share 1, "gamma[0]"
// and "delta[0]" share 2, "p_gamma[0]" and "p_delta[0]" share 25).
int32_t nameToBaseId(const std::string& name) {
    static const std::unordered_map<std::string, int32_t> kTable = {
        {"mipmaps", 0},
        {"generate", 1},
        {"gamma[0]", 2},
        {"gamma[1]", 4},
        {"gamma[2]", 5},
        {"gamma[3]", 6},
        {"gamma[4]", 7},
        {"delta[0]", 2},
        {"delta[1]", 8},
        {"delta[2]", 9},
        {"delta[3]", 10},
        {"delta[4]", 11},
        {"delta[5]", 3},
        {"delta[6]", 16},
        {"delta[7]", 17},
        {"delta[8]", 18},
        {"delta[9]", 19},
        {"alpha[0]", 12},
        {"alpha[1]", 13},
        {"alpha[2]", 14},
        {"alpha[3]", 15},
        {"beta[0]", 20},
        {"beta[1]", 21},
        {"beta[2]", 22},
        {"beta[3]", 23},
        {"beta[4]", 24},
        {"p_mipmaps", 0},
        {"p_generate", 1},
        {"p_gamma[0]", 25},
        {"p_gamma[1]", 27},
        {"p_gamma[2]", 28},
        {"p_gamma[3]", 29},
        {"p_gamma[4]", 30},
        {"p_delta[0]", 25},
        {"p_delta[1]", 31},
        {"p_delta[2]", 32},
        {"p_delta[3]", 33},
        {"p_delta[4]", 34},
        {"p_delta[5]", 26},
        {"p_delta[6]", 39},
        {"p_delta[7]", 40},
        {"p_delta[8]", 41},
        {"p_delta[9]", 42},
        {"p_alpha[0]", 35},
        {"p_alpha[1]", 36},
        {"p_alpha[2]", 37},
        {"p_alpha[3]", 38},
        {"p_beta[0]", 43},
        {"p_beta[1]", 44},
        {"p_beta[2]", 45},
        {"p_beta[3]", 46},
        {"p_beta[4]", 47},
    };
    const auto it = kTable.find(name);
    return it == kTable.end() ? -1 : it->second;
}


}  // namespace tlsfg
