// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/android_shader_loader.cpp, GPL-3.0.
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Extraction of the precompiled SPIR-V shader sets from the user's
// Lossless.dll and the loader handed to LSFG_3_1[P]::initialize.
//
// Taken from the reference: the RT_RCDATA resource callback, the SPIR-V
// version cap, the dense binding rewrite and the framegen name table. Not
// taken: the Direct3D bytecode translation path and its resource range. Only
// the precompiled SPIR-V sets are used:
//   FP32: ids 353..400 (kFp32FirstId..kFp32LastId), all 48 required.
//   FP16: ids 304..351 (kFp16FirstId..kFp16LastId), optional and all-or-nothing.
//
// Id scheme. The reference keys every shader by its Direct3D bytecode resource
// id D and derives FP16 = D + 49 and FP32 = D + 98. Here a shader is keyed by
// its base index b = D - (first Direct3D id), so b runs 0..47 and
//   FP32 id = 353 + b  (== D + 98),
//   FP16 id = 304 + b  (== D + 49).
// The name table below is the reference table with each id rebased that way
// (e.g. "mipmaps" 0, "generate" 1, "p_beta[4]" 47); both ranges stay inside
// 353..400 and 304..351 exactly as the reference range guards require.
//
// The DLL is read into a std::vector and parsed with ParsePEFromPointer (the
// parser keeps pointers into that vector, which outlives the parsed PE). No
// file descriptor or mapping stays alive after extractShaders returns.

#include "fg_shader_loader.h"

#include "fg_types.h"

#include <pe-parse/parse.h>

#include <android/log.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <exception>
#include <fstream>
#include <limits>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace efg {

namespace {

constexpr const char* kTag = "TatakaiFG.shaders";

#define EFG_SL_LOGE(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)
#define EFG_SL_LOGW(...) __android_log_print(ANDROID_LOG_WARN, kTag, __VA_ARGS__)
#define EFG_SL_LOGI(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)

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

// Reads a whole file into `out`. No mapping and no descriptor kept.
bool readWholeFile(const std::string& path, std::vector<uint8_t>& out) {
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

bool ensureDir(const std::string& path) {
    if (::mkdir(path.c_str(), 0700) == 0) return true;
    if (errno != EEXIST) return false;
    struct stat st {};
    return ::stat(path.c_str(), &st) == 0 && S_ISDIR(st.st_mode);
}

// Atomic write: <path>.tmp, fsync, rename over <path>. The temporary file is
// removed on any failure.
bool writeFileAtomic(const std::string& path, const std::vector<uint8_t>& data) {
    const std::string tmp = path + ".tmp";
    const int fd = ::open(tmp.c_str(), O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
    if (fd < 0) return false;
    bool ok = true;
    size_t off = 0;
    while (ok && off < data.size()) {
        const ssize_t n = ::write(fd, data.data() + off, data.size() - off);
        if (n < 0) {
            if (errno == EINTR) continue;
            ok = false;
        } else {
            off += static_cast<size_t>(n);
        }
    }
    if (ok && ::fsync(fd) != 0) ok = false;
    if (::close(fd) != 0) ok = false;
    if (ok && ::rename(tmp.c_str(), path.c_str()) != 0) ok = false;
    if (!ok) ::unlink(tmp.c_str());
    return ok;
}

std::string spvPath(const std::string& dir, const char* sub, int32_t id) {
    return dir + "/" + sub + "/" + std::to_string(id) + ".spv";
}

// Removes the known FP16 file names under outDir/fp16 (only <id>.spv for the
// 48 FP16 ids, never the directory) so a stale set from an earlier DLL cannot
// be mixed with a new FP32 set.
void removeFp16Set(const std::string& outDir) {
    for (int32_t b = 0; b < kShaderCount; ++b) {
        const std::string p = spvPath(outDir, "fp16", kFp16FirstId + b);
        ::unlink(p.c_str());
    }
}

// True when the file exists, is non-empty and starts with the SPIR-V magic.
bool isSpirvFile(const std::string& path) {
    struct stat st {};
    if (::stat(path.c_str(), &st) != 0 || !S_ISREG(st.st_mode) ||
        st.st_size < static_cast<off_t>(kSpirvHeaderBytes)) {
        return false;
    }
    std::ifstream f(path, std::ios::binary);
    if (!f) return false;
    std::array<uint8_t, 4> head{};
    if (!f.read(reinterpret_cast<char*>(head.data()), static_cast<std::streamsize>(head.size()))) {
        return false;
    }
    return head == kSpirvMagic;
}

bool setComplete(const std::string& dir, const char* sub, int32_t firstId) {
    for (int32_t b = 0; b < kShaderCount; ++b) {
        if (!isSpirvFile(spvPath(dir, sub, firstId + b))) return false;
    }
    return true;
}

// Loads one cached shader; empty on any failure.
std::vector<uint8_t> loadSpv(const std::string& path) {
    std::vector<uint8_t> out;
    if (!readWholeFile(path, out) || !hasSpirvMagic(out)) return {};
    return out;
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

int32_t extractShaders(const std::string& dllPath, const std::string& outDir) {
    std::vector<uint8_t> dll;
    if (!readWholeFile(dllPath, dll) || dll.empty() ||
        dll.size() > static_cast<size_t>(std::numeric_limits<uint32_t>::max())) {
        EFG_SL_LOGE("cannot read DLL %s", dllPath.c_str());
        return FgResult::DLL_UNREADABLE;
    }

    BlobMap blobs;
    {
        peparse::parsed_pe* pe =
            peparse::ParsePEFromPointer(dll.data(), static_cast<uint32_t>(dll.size()));
        if (pe == nullptr) {
            EFG_SL_LOGE("ParsePEFromPointer failed: %s", peparse::GetPEErrString().c_str());
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
    // The parsed PE is gone; the DLL bytes are no longer needed either.
    std::vector<uint8_t>().swap(dll);

    // FP32: all required.
    std::vector<std::vector<uint8_t>> fp32;
    int32_t bad = 0;
    const int32_t rc32 = prepareSet(blobs, kFp32FirstId, fp32, bad);
    if (rc32 != FgResult::OK) {
        EFG_SL_LOGE("FP32 SPIR-V id %d %s", bad,
                    rc32 == FgResult::MISSING_SPIRV ? "missing or not SPIR-V" : "binding rewrite failed");
        return rc32;
    }

    // FP16: optional, all-or-nothing.
    std::vector<std::vector<uint8_t>> fp16;
    int32_t bad16 = 0;
    bool haveFp16 = prepareSet(blobs, kFp16FirstId, fp16, bad16) == FgResult::OK;
    if (!haveFp16) {
        EFG_SL_LOGW("FP16 SPIR-V set unavailable (id %d); FP16 skipped", bad16);
        fp16.clear();
    }

    if (!ensureDir(outDir) || !ensureDir(outDir + "/fp32")) {
        EFG_SL_LOGE("cannot create %s/fp32", outDir.c_str());
        return FgResult::WRITE_FAILED;
    }
    int32_t written = 0;
    for (int32_t b = 0; b < kShaderCount; ++b) {
        const std::string p = spvPath(outDir, "fp32", kFp32FirstId + b);
        if (!writeFileAtomic(p, fp32[static_cast<size_t>(b)])) {
            EFG_SL_LOGE("cannot write %s", p.c_str());
            return FgResult::WRITE_FAILED;
        }
        ++written;
    }

    // Any previous FP16 set is dropped first so the result is either a full
    // fresh set or none. FP16 write failures do not fail the extraction.
    removeFp16Set(outDir);
    if (haveFp16) {
        if (!ensureDir(outDir + "/fp16")) {
            EFG_SL_LOGW("cannot create %s/fp16; FP16 skipped", outDir.c_str());
            haveFp16 = false;
        }
        for (int32_t b = 0; haveFp16 && b < kShaderCount; ++b) {
            const std::string p = spvPath(outDir, "fp16", kFp16FirstId + b);
            if (!writeFileAtomic(p, fp16[static_cast<size_t>(b)])) {
                EFG_SL_LOGW("cannot write %s; FP16 skipped", p.c_str());
                removeFp16Set(outDir);
                haveFp16 = false;
            }
        }
    }

    EFG_SL_LOGI("extracted %d FP32 shaders, FP16 %s, into %s", written,
                haveFp16 ? "present" : "absent", outDir.c_str());
    return written;
}

bool probeShaders(const std::string& dir) {
    return setComplete(dir, "fp32", kFp32FirstId);
}

// Loader contract (framegen src/pool/shaderpool.cpp ShaderPool::getShader):
// an empty vector makes framegen throw std::runtime_error("Shader code is
// empty: <name>"), which propagates out of LSFG initialize to FgEngine. So the
// loader never throws: an unknown name or an unreadable file yields an empty
// vector (logged), and the error surfaces as one well-defined exception inside
// framegen. FP16 is used only when allowFp16 and the FP16 set was complete
// when the loader was made; a single FP16 file that later fails to load falls
// back to its FP32 twin.
ShaderLoader makeLoader(const std::string& dir, bool allowFp16) {
    const bool useFp16 = allowFp16 && setComplete(dir, "fp16", kFp16FirstId);
    EFG_SL_LOGI("shader loader: %s set from %s", useFp16 ? "FP16" : "FP32", dir.c_str());
    return [dir, useFp16](const std::string& name) -> std::vector<uint8_t> {
        try {
            const int32_t b = nameToBaseId(name);
            if (b < 0) {
                EFG_SL_LOGE("unknown shader name '%s'", name.c_str());
                return {};
            }
            std::vector<uint8_t> spirv;
            if (useFp16) {
                spirv = loadSpv(spvPath(dir, "fp16", kFp16FirstId + b));
                if (spirv.empty()) {
                    EFG_SL_LOGW("FP16 shader '%s' unreadable; using FP32", name.c_str());
                }
            }
            if (spirv.empty()) {
                spirv = loadSpv(spvPath(dir, "fp32", kFp32FirstId + b));
            }
            if (spirv.empty()) {
                EFG_SL_LOGE("shader '%s' (FP32 id %d) missing from %s", name.c_str(),
                            kFp32FirstId + b, dir.c_str());
            }
            return spirv;
        } catch (const std::exception& e) {
            EFG_SL_LOGE("shader '%s' load failed: %s", name.c_str(), e.what());
            return {};
        } catch (...) {
            EFG_SL_LOGE("shader '%s' load failed", name.c_str());
            return {};
        }
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

}  // namespace efg
