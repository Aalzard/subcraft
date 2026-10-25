#include "texture_export.h"

#include <substance/framework/framework.h>

#include <algorithm>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

namespace {

template <class Container>
void loadFile(Container& container, const std::string& path, bool binary)
{
    std::ifstream is(path.c_str(), std::ios::in | (binary ? std::ios::binary : std::ios::openmode(0)));
    if (!is)
        return;
    is.seekg(0, std::ios::end);
    container.resize(static_cast<size_t>(is.tellg()));
    is.seekg(0, std::ios::beg);
    is.read(reinterpret_cast<char*>(&container[0]), static_cast<std::streamsize>(container.size()));
}

int log2Size(unsigned int size)
{
    if (size < 16 || size > 8192)
        return 10;
    unsigned int v = size;
    int log = 0;
    while (v > 1)
    {
        if (v & 1)
            return 10;
        v >>= 1;
        ++log;
    }
    return log;
}

template <typename StringLike>
std::string jsonEscape(const StringLike& s)
{
    std::string out;
    out.reserve(s.size() + 8);
    for (typename StringLike::const_iterator it = s.begin(); it != s.end(); ++it)
    {
        const char c = *it;
        switch (c)
        {
        case '\\': out += "\\\\"; break;
        case '"': out += "\\\""; break;
        case '\n': out += "\\n"; break;
        default: out += c;
        }
    }
    return out;
}

void setOutputSize(SubstanceAir::GraphInstance* graph, int log2wh)
{
    using namespace SubstanceAir;
    const GraphInstance::Inputs& inputs = graph->getInputs();
    for (GraphInstance::Inputs::const_iterator inite = inputs.begin(); inite != inputs.end(); ++inite)
    {
        InputInstanceBase* const input = *inite;
        if (input->mDesc.mIdentifier == "$outputsize" && input->mDesc.mType == Substance_IOType_Integer2)
            static_cast<InputInstanceInt2*>(input)->setValue(Vec2Int(log2wh, log2wh));
        else if (input->mDesc.mIdentifier == "normal_format" && input->mDesc.mType == Substance_IOType_Integer)
            static_cast<InputInstanceInt*>(input)->setValue(1);
    }
}

} // namespace

int main(int argc, char** argv)
{
    if (argc < 3)
    {
        std::cerr << "Usage: " << argv[0] << " <sbsar> <out_dir> [size_px] [graph_index] [--set key=val ...]\n";
        return 1;
    }

    const std::string sbsarPath = argv[1];
    const std::string outDir = argv[2];
    const unsigned int sizePx = argc >= 4 ? static_cast<unsigned int>(std::strtoul(argv[3], nullptr, 10)) : 1024u;
    const size_t graphIndex = argc >= 5 ? static_cast<size_t>(std::strtoul(argv[4], nullptr, 10)) : 0u;
    const int log2wh = log2Size(sizePx);

    std::vector<std::pair<std::string, std::string> > overrides;
    for (int i = 5; i < argc; ++i)
    {
        std::string arg = argv[i];
        if (arg == "--set" && i + 1 < argc)
        {
            std::string kv = argv[++i];
            size_t eq = kv.find('=');
            if (eq != std::string::npos)
                overrides.push_back(std::make_pair(kv.substr(0, eq), kv.substr(eq + 1)));
        }
    }

    std::vector<unsigned char> archiveData;
    loadFile(archiveData, sbsarPath, true);
    if (archiveData.empty())
    {
        std::cerr << "Unable to read: " << sbsarPath << "\n";
        return 2;
    }

    using namespace SubstanceAir;

    PackageDesc packagedesc(&archiveData[0], archiveData.size());
    if (!packagedesc.isValid())
    {
        std::cerr << "Invalid SBSAR package.\n";
        return 3;
    }

    GraphInstances instances;
    instantiate(instances, packagedesc);
    if (instances.empty())
    {
        std::cerr << "No graphs in package.\n";
        return 4;
    }
    if (graphIndex >= instances.size())
    {
        std::cerr << "graph_index " << graphIndex << " out of range (0.." << (instances.size() - 1) << ")\n";
        return 5;
    }

    GraphInstances renderSet;
    renderSet.push_back(instances[graphIndex]);
    setOutputSize(renderSet[0].get(), log2wh);

    for (size_t oi = 0; oi < overrides.size(); ++oi)
    {
        const GraphInstance::Inputs& inputs = renderSet[0]->getInputs();
        for (GraphInstance::Inputs::const_iterator inite = inputs.begin(); inite != inputs.end(); ++inite)
        {
            InputInstanceBase* const input = *inite;
            if (input->mDesc.mIdentifier.c_str() == overrides[oi].first && input->mDesc.isNumerical())
            {
                static_cast<InputInstanceNumericalBase*>(input)->setValue(overrides[oi].second.c_str());
                break;
            }
        }
    }

    const GraphInstance::Outputs& preOutputs = renderSet[0]->getOutputs();
    for (GraphInstance::Outputs::const_iterator outite = preOutputs.begin(); outite != preOutputs.end(); ++outite)
    {
        OutputInstance* const out = *outite;
        if (!out->mDesc.isImage())
            continue;
        if ((out->mDesc.mFormat & Substance_PF_MASK_RAWFormat) != Substance_PF_RAW ||
            (out->mDesc.mFormat & Substance_PF_MASK_RAWChannels) == Substance_PF_L ||
            (out->mDesc.mFormat & Substance_PF_MASK_RAWChannels) == Substance_PF_RGB)
        {
            OutputFormat fmt;
            fmt.format = Substance_PF_RGBA | (out->mDesc.mFormat & Substance_PF_sRGB);
            fmt.mipmapLevelsCount = OutputFormat::MipmapNone;
            out->overrideFormat(fmt);
        }
    }

    Renderer renderer;
    renderer.push(renderSet);
    renderer.run();

    const string& pkgurl = renderSet[0]->mDesc.mPackageUrl;
    const SubstanceAir::string graphName = pkgurl.substr(pkgurl.find_last_of('/') + 1);

    std::ostringstream manifest;
    manifest << "{\n"
             << "  \"sbsar\": \"" << jsonEscape(sbsarPath) << "\",\n"
             << "  \"graph\": \"" << jsonEscape(graphName) << "\",\n"
             << "  \"graph_index\": " << graphIndex << ",\n"
             << "  \"size\": " << (1u << log2wh) << ",\n"
             << "  \"graphs\": [\n";
    for (size_t i = 0; i < instances.size(); ++i)
    {
        const string& url = instances[i]->mDesc.mPackageUrl;
        const SubstanceAir::string name = url.substr(url.find_last_of('/') + 1);
        manifest << "    {\"index\": " << i << ", \"name\": \"" << jsonEscape(name) << "\"}"
                 << (i + 1 < instances.size() ? ",\n" : "\n");
    }
    manifest << "  ],\n  \"inputs\": [\n";
    {
        bool firstIn = true;
        const GraphInstance::Inputs& inputs = renderSet[0]->getInputs();
        for (GraphInstance::Inputs::const_iterator inite = inputs.begin(); inite != inputs.end(); ++inite)
        {
            InputInstanceBase* const input = *inite;
            if (!input->mDesc.isNumerical())
                continue;
            const SubstanceAir::string& id = input->mDesc.mIdentifier;
            if (!id.empty() && id[0] == '$' && id != "$randomseed")
                continue;
            if (!input->IsVisible())
                continue;

            const char* typeStr = nullptr;
            switch (input->mDesc.mType)
            {
            case Substance_IOType_Float: typeStr = "float"; break;
            case Substance_IOType_Float2: typeStr = "float2"; break;
            case Substance_IOType_Float3: typeStr = "float3"; break;
            case Substance_IOType_Float4: typeStr = "float4"; break;
            case Substance_IOType_Integer: typeStr = "int"; break;
            case Substance_IOType_Integer2: typeStr = "int2"; break;
            case Substance_IOType_Integer3: typeStr = "int3"; break;
            case Substance_IOType_Integer4: typeStr = "int4"; break;
            default: break;
            }
            if (!typeStr)
                continue;

            const char* widgetStr = "slider";
            switch (input->mDesc.mGuiWidget)
            {
            case Input_Angle: widgetStr = "angle"; break;
            case Input_Color: widgetStr = "color"; break;
            case Input_Togglebutton: widgetStr = "toggle"; break;
            case Input_Enumbuttons:
            case Input_Combobox: widgetStr = "combobox"; break;
            case Input_Position: widgetStr = "position"; break;
            default: widgetStr = "slider"; break;
            }

            InputInstanceNumericalBase* const num = static_cast<InputInstanceNumericalBase*>(input);
            const SubstanceAir::string label = input->mDesc.mLabel.empty() ? id : input->mDesc.mLabel;
            if (!firstIn)
                manifest << ",\n";
            firstIn = false;

            manifest << "    {\"identifier\": \"" << jsonEscape(id) << "\""
                     << ", \"label\": \"" << jsonEscape(label) << "\""
                     << ", \"group\": \"" << jsonEscape(input->mDesc.mGuiGroup) << "\""
                     << ", \"kind\": \"" << typeStr << "\""
                     << ", \"widget\": \"" << widgetStr << "\""
                     << ", \"value\": \"" << jsonEscape(num->getValueAsString()) << "\"";

            if (input->mDesc.mType == Substance_IOType_Float)
            {
                const InputDescFloat& d = static_cast<const InputDescFloat&>(input->mDesc);
                manifest << ", \"min\": " << d.mMinValue
                         << ", \"max\": " << d.mMaxValue
                         << ", \"default\": \"" << d.mDefaultValue << "\"";
            }
            else if (input->mDesc.mType == Substance_IOType_Integer)
            {
                const InputDescInt& d = static_cast<const InputDescInt&>(input->mDesc);
                manifest << ", \"min\": " << d.mMinValue
                         << ", \"max\": " << d.mMaxValue
                         << ", \"default\": \"" << d.mDefaultValue << "\"";
                if (!d.mEnumValues.empty())
                {
                    manifest << ", \"enum_values\": [";
                    for (size_t ei = 0; ei < d.mEnumValues.size(); ++ei)
                    {
                        if (ei > 0) manifest << ", ";
                        manifest << "{\"value\": " << d.mEnumValues[ei].first
                                 << ", \"label\": \"" << jsonEscape(d.mEnumValues[ei].second) << "\"}";
                    }
                    manifest << "]";
                }
            }
            manifest << "}";
        }
        if (!firstIn)
            manifest << "\n";
    }
    manifest << "  ],\n  \"outputs\": [\n";

    bool firstOut = true;
    size_t written = 0;
    const GraphInstance::Outputs& outputs = renderSet[0]->getOutputs();
    for (GraphInstance::Outputs::const_iterator outite = outputs.begin(); outite != outputs.end(); ++outite)
    {
        OutputInstance::Result result((*outite)->grabResult());
        if (result.get() == nullptr || !result->isImage())
            continue;

        RenderResultImage* const resimg = dynamic_cast<RenderResultImage*>(result.get());
        const SubstanceAir::string identifier = (*outite)->mDesc.mIdentifier;
        SubstanceAir::string safeId = identifier;
        for (char& c : safeId)
        {
            if (c == '/' || c == '\\' || c == ':' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|')
                c = '_';
        }
        const SubstanceAir::string fileName = safeId + ".tga";
        const SubstanceAir::string filePath = SubstanceAir::string(outDir.c_str()) + "/" + fileName;

        if (!writeTextureTga(filePath.c_str(), resimg->getTexture()))
        {
            std::cerr << "Skipped unsupported format for output " << identifier
                      << " (pf=0x" << std::hex << static_cast<unsigned int>(resimg->getTexture().pixelFormat)
                      << ", w=" << std::dec << resimg->getTexture().level0Width
                      << ", h=" << resimg->getTexture().level0Height
                      << ", buf=" << (resimg->getTexture().buffer != nullptr ? 1 : 0) << ")\n";
            continue;
        }

        if (!firstOut)
            manifest << ",\n";
        firstOut = false;
        manifest << "    {\"file\": \"" << jsonEscape(fileName) << "\", \"identifier\": \"" << jsonEscape(identifier)
                 << "\"}";
        ++written;
    }
    manifest << "\n  ]\n}\n";

    if (written == 0)
    {
        std::cerr << "No image outputs written.\n";
        return 6;
    }

    const std::string manifestPath = outDir + "/manifest.json";
    std::ofstream mf(manifestPath.c_str(), std::ios::out | std::ios::trunc);
    if (!mf)
    {
        std::cerr << "Unable to write manifest: " << manifestPath << "\n";
        return 7;
    }
    mf << manifest.str();
    return 0;
}
