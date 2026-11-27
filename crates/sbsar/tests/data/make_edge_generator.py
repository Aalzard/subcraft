# Builds a tiny Substance generator (.sbsar): image inputs curvature + ambient_occlusion,
# one grayscale output "mask" = edges from curvature, darkened in occluded areas.
# Uses open-sbsar's tools/write.py (MIT/Apache).
import sys, os, tempfile, subprocess
sys.path.insert(0, "/tmp/osb/tools")
import write as W
out = sys.argv[1]
CURV, AO = 0x11110001, 0x11110002
nodes = [
    W.Record([W.opword(16, 1), CURV]),                                   # 0 curvature
    W.levels(0, inlow=[0.55, 0.55, 0.55, 0.0], inhigh=[0.75, 0.75, 0.75, 1.0]),  # 1 edges
    W.Record([W.opword(16, 1), AO]),                                     # 2 ao
    W.blend(1, 2, mode=3, opacity=1.0),  # 3 multiply
]
outs = [(0x22220001, "mask", 3, True, "mask")]
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
<graphs count="1"><graph pkgurl="pkg://edge_generator" label="Edge Generator">
<sbspresets count="0"></sbspresets><guigroups/>
<outputs count="1">%s</outputs>
<inputs count="3">
<input uid="%d" identifier="$randomseed" type="4" default="0" alteroutputs="" alternodes="0"/>%s%s
</inputs></graph></graphs></sbsdescription>
''' % (asmuid, W.COOKER_BUILD, o, W.SEED_UID, img_in(CURV, "curvature", "Curvature"), img_in(AO, "ambient_occlusion", "Ambient Occlusion"))
with tempfile.TemporaryDirectory() as d:
    c = os.path.join(d, "assemblies", "content", "0000"); os.makedirs(c)
    open(os.path.join(c, "edge_generator.sbsasm"), "wb").write(asm)
    open(os.path.join(c, "edge_generator.xml"), "w").write(xml)
    if os.path.exists(out): os.remove(out)
    subprocess.run(["7z", "a", "-mx=0", os.path.abspath(out), "assemblies"], cwd=d, check=True, stdout=subprocess.DEVNULL)
print("wrote", out)
