#include "texture_export.h"

#include <substance/pixelformat.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <vector>

static void writeTgaHeader(std::ostream& out, uint16_t width, uint16_t height, uint8_t bpp)
{
    const uint8_t header[18] = {
        0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        static_cast<uint8_t>(width & 0xff),
        static_cast<uint8_t>((width >> 8) & 0xff),
        static_cast<uint8_t>(height & 0xff),
        static_cast<uint8_t>((height >> 8) & 0xff),
        bpp,
        0x20,
    };
    out.write(reinterpret_cast<const char*>(header), sizeof(header));
}

static inline uint8_t floatToByte(float f)
{
    if (!(f > 0.f))
        return 0;
    if (f >= 1.f)
        return 255;
    return static_cast<uint8_t>(f * 255.f + 0.5f);
}

static inline float halfToFloat(uint16_t h)
{
    const uint32_t sign = static_cast<uint32_t>(h & 0x8000u) << 16;
    const uint32_t exp = (h >> 10) & 0x1fu;
    const uint32_t mant = h & 0x03ffu;
    uint32_t bits = 0;
    if (exp == 0)
    {
        if (mant == 0)
        {
            bits = sign;
        }
        else
        {
            float v = std::ldexp(static_cast<float>(mant), -24);
            return (sign != 0) ? -v : v;
        }
    }
    else if (exp == 31)
    {
        bits = sign | 0x7f800000u | (mant << 13);
    }
    else
    {
        bits = sign | ((exp + 112u) << 23) | (mant << 13);
    }
    float out = 0.f;
    std::memcpy(&out, &bits, sizeof(float));
    return out;
}

static inline void channelIndices(unsigned char order, int& rIdx, int& gIdx, int& bIdx, int& aIdx)
{
    switch (order)
    {
    case Substance_ChanOrder_BGRA:
        bIdx = 0; gIdx = 1; rIdx = 2; aIdx = 3;
        break;
    case Substance_ChanOrder_ABGR:
        aIdx = 0; bIdx = 1; gIdx = 2; rIdx = 3;
        break;
    case Substance_ChanOrder_ARGB:
        aIdx = 0; rIdx = 1; gIdx = 2; bIdx = 3;
        break;
    case Substance_ChanOrder_RGBA:
    default:
        rIdx = 0; gIdx = 1; bIdx = 2; aIdx = 3;
        break;
    }
}

template <typename SampleT, typename ToByteFn>
static bool exportRaw(
    std::ostream& out,
    const SubstanceTexture& tex,
    unsigned int width,
    unsigned int height,
    unsigned int rawChannels,
    ToByteFn toByte)
{
    const SampleT* src = static_cast<const SampleT*>(tex.buffer);
    const size_t rowBytes = static_cast<size_t>(width) * 4u;
    std::vector<uint8_t> row(rowBytes);

    int rIdx = 0, gIdx = 1, bIdx = 2, aIdx = 3;
    channelIndices(tex.channelsOrder, rIdx, gIdx, bIdx, aIdx);

    const size_t stride = (rawChannels == Substance_PF_L)
        ? 1u
        : (rawChannels == Substance_PF_RGB ? 3u : 4u);

    for (unsigned int y = 0; y < height; ++y)
    {
        const SampleT* line = src + static_cast<size_t>(y) * width * stride;
        for (unsigned int x = 0; x < width; ++x)
        {
            uint8_t r = 0, g = 0, b = 0, a = 255;
            if (rawChannels == Substance_PF_L)
            {
                r = g = b = toByte(line[0]);
            }
            else if (rawChannels == Substance_PF_RGB)
            {
                r = toByte(line[0]);
                g = toByte(line[1]);
                b = toByte(line[2]);
            }
            else if (rawChannels == Substance_PF_RGBx)
            {
                r = toByte(line[rIdx]);
                g = toByte(line[gIdx]);
                b = toByte(line[bIdx]);
            }
            else // Substance_PF_RGBA
            {
                r = toByte(line[rIdx]);
                g = toByte(line[gIdx]);
                b = toByte(line[bIdx]);
                a = toByte(line[aIdx]);
            }
            row[x * 4 + 0] = b;
            row[x * 4 + 1] = g;
            row[x * 4 + 2] = r;
            row[x * 4 + 3] = a;
            line += stride;
        }
        out.write(reinterpret_cast<const char*>(row.data()), rowBytes);
    }
    return true;
}

bool writeTextureTga(const char* filename, const SubstanceTexture& texture)
{
    const unsigned int width = texture.level0Width;
    const unsigned int height = texture.level0Height;
    if (width == 0 || height == 0 || texture.buffer == nullptr)
        return false;

    const unsigned int pf = texture.pixelFormat & ~Substance_PF_sRGB;
    if ((pf & Substance_PF_MASK_RAWFormat) != Substance_PF_RAW)
        return false;

    const unsigned int rawChannels = pf & Substance_PF_MASK_RAWChannels;
    const unsigned int precision = pf & Substance_PF_MASK_RAWPrecision;

    std::ofstream out(filename, std::ios::binary);
    if (!out)
        return false;

    writeTgaHeader(out, static_cast<uint16_t>(width), static_cast<uint16_t>(height), 32);

    switch (precision)
    {
    case Substance_PF_8I:
        return exportRaw<uint8_t>(out, texture, width, height, rawChannels, [](uint8_t v) -> uint8_t { return v; });
    case Substance_PF_16I:
        return exportRaw<uint16_t>(out, texture, width, height, rawChannels, [](uint16_t v) -> uint8_t {
            return static_cast<uint8_t>((static_cast<uint32_t>(v) * 255u + 32767u) / 65535u);
        });
    case Substance_PF_16F:
        return exportRaw<uint16_t>(out, texture, width, height, rawChannels, [](uint16_t v) -> uint8_t {
            return floatToByte(halfToFloat(v));
        });
    case Substance_PF_32F:
        return exportRaw<float>(out, texture, width, height, rawChannels, [](float v) -> uint8_t {
            return floatToByte(v);
        });
    default:
        return false;
    }
}
