# Builds a tiny Substance filter (.sbsar): image inputs basecolor (color) and roughness
# (grayscale), outputs basecolor and roughness = each input inverted. Painter-style filters
# read the layer stack below through such channel-named inputs.
# Uses open-sbsar's tools/write.py (MIT/Apache), from a clone at /tmp/osb; needs 7z.
import sys, os, tempfile, subprocess
sys.path.insert(0, "/tmp/osb/tools")
import write as W
out = sys.argv[1]
BASE, ROUGH = 0x11120001, 0x11120002
nodes = [
    W.Record([W.opword(16, 1), BASE]),                                                   # 0 basecolor in
    W.levels(0, outlow=[1.0, 1.0, 1.0, 1.0], outhigh=[0.0, 0.0, 0.0, 1.0]),               # 1 inverted
    W.Record([W.opword(16, 1), ROUGH]),                                                  # 2 roughness in
    W.levels(2, outlow=[1.0, 1.0, 1.0, 1.0], outhigh=[0.0, 0.0, 0.0, 1.0]),               # 3 inverted
]
outs = [(0x22230001, "basecolor", 1, False, "baseColor"), (0x22230002, "roughness", 3, True, "roughness")]
asm, asmuid = W.assemble(nodes, [(u, i, 0xC0 if g else 0) for u, _, i, g, _ in outs])
o = "".join('''
<output uid="%d" identifier="%s" type="5" format="%d" width="256" height="256" mipmaps="0">
<outputgui label="%s" typegui="image"><channels names="%s"/></outputgui></output>''' % (u, ident, 12 if g else 0, ident, usage) for u, ident, _, g, usage in outs)
def img_in(uid, ident, label):
    return '''
<input uid="%d" identifier="%s" type="5" default="" alteroutputs="" alternodes="0">
<inputgui label="%s" widget="image"/></input>''' % (uid, ident, label)
xml = '''<?xml version="1.0" encoding="UTF-8"?>
<sbsdescription formatversion="2.1" asmuid="%d" cookerbuild="%d" content="full">
<graphs count="1"><graph pkgurl="pkg://invert_filter" label="Invert Filter">
<sbspresets count="0"></sbspresets><guigroups/>
<outputs count="2">%s</outputs>
<inputs count="3">
<input uid="%d" identifier="$randomseed" type="4" default="0" alteroutputs="" alternodes="0"/>%s%s
</inputs></graph></graphs></sbsdescription>
''' % (asmuid, W.COOKER_BUILD, o, W.SEED_UID, img_in(BASE, "basecolor", "Base Color"), img_in(ROUGH, "roughness", "Roughness"))
with tempfile.TemporaryDirectory() as d:
    c = os.path.join(d, "assemblies", "content", "0000"); os.makedirs(c)
    open(os.path.join(c, "invert_filter.sbsasm"), "wb").write(asm)
    open(os.path.join(c, "invert_filter.xml"), "w").write(xml)
    if os.path.exists(out): os.remove(out)
    subprocess.run(["7z", "a", "-mx=0", os.path.abspath(out), "assemblies"], cwd=d, check=True, stdout=subprocess.DEVNULL)
print("wrote", out)
