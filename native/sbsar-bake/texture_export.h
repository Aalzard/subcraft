#ifndef SUBCRAFT_TEXTURE_EXPORT_H
#define SUBCRAFT_TEXTURE_EXPORT_H

#include <substance/texture.h>

/** Write level-0 as uncompressed 32-bit TGA (BGRA). Returns false if format unsupported. */
bool writeTextureTga(const char* filename, const SubstanceTexture& texture);

#endif
