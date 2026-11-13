# SubCraft -> Blender bridge. Run by SubCraft as:
#   blender --python subcraft_bridge.py -- path/to/bridge.json
# Imports the low poly SubCraft exported, wires its textures into a Principled BSDF, and adds a
# "SubCraft" tab in the 3D view's sidebar (N) with "Send to SubCraft", which writes the selected
# object(s) back as the high poly for baking. Works with Blender 3.6 and newer.

import json
import os
import sys

import bpy

BRIDGE_VERSION = 1
PROP = "subcraft_bridge_manifest"


def log(msg):
    print("[SubCraft] " + msg, flush=True)


def manifest_path_from_argv():
    argv = sys.argv
    if "--" in argv:
        rest = argv[argv.index("--") + 1:]
        if rest:
            return rest[0]
    return None


def load_manifest(path):
    with open(path, "r", encoding="utf-8") as f:
        m = json.load(f)
    if int(m.get("version", 0)) > BRIDGE_VERSION:
        raise RuntimeError("this bridge.json was written by a newer SubCraft; update the script")
    return m


# ----- import -----------------------------------------------------------------------------------

def clear_default_scene():
    """Remove the startup cube when the scene is Blender's untouched default."""
    names = sorted(o.name for o in bpy.data.objects)
    if names == ["Camera", "Cube", "Light"]:
        cube = bpy.data.objects["Cube"]
        mesh = cube.data
        bpy.data.objects.remove(cube, do_unlink=True)
        if mesh is not None and mesh.users == 0:
            bpy.data.meshes.remove(mesh)


def import_obj(path):
    before = set(bpy.data.objects)
    if hasattr(bpy.ops.wm, "obj_import"):
        # SubCraft is +Y up, -Z forward: the importer's defaults; "Send to SubCraft" uses the same.
        bpy.ops.wm.obj_import(filepath=path, forward_axis="NEGATIVE_Z", up_axis="Y")
    else:
        bpy.ops.import_scene.obj(filepath=path, axis_forward="-Z", axis_up="Y")
    return [o for o in bpy.data.objects if o not in before and o.type == "MESH"]


def find_material(obj, name):
    for slot in obj.material_slots:
        m = slot.material
        if m is None:
            continue
        if m.name == name or (m.name.startswith(name + ".") and m.name[len(name) + 1:].isdigit()):
            return m
    return bpy.data.materials.get(name)


# ----- material -----------------------------------------------------------------------------------

def bsdf_input(bsdf, *names):
    for n in names:
        if n in bsdf.inputs:
            return bsdf.inputs[n]
    return None


def load_image(path, srgb, udim):
    img = bpy.data.images.load(path, check_existing=True)
    if udim:
        try:
            img.source = "TILED"
        except Exception as e:  # older builds or an unexpected file name
            log("UDIM tiles not loaded for %s: %s" % (path, e))
    img.colorspace_settings.name = "sRGB" if srgb else "Non-Color"
    return img


def build_material(mat, textures):
    try:
        mat.use_nodes = True  # always on (and deprecated) in Blender 5
    except Exception:
        pass
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    out.location = (900, 0)
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.location = (500, 0)
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    uv = nt.nodes.new("ShaderNodeTexCoord")
    uv.location = (-1100, 0)

    images = {}  # (path, srgb) -> image texture node
    separates = {}  # path -> separate color node
    row = [0]

    def image_node(t):
        key = (t["path"], bool(t["srgb"]) and t.get("component") is None)
        if key in images:
            return images[key]
        node = nt.nodes.new("ShaderNodeTexImage")
        node.image = load_image(t["path"], key[1], t.get("udim", False))
        node.label = os.path.basename(t["path"])
        node.location = (-800, 400 - row[0] * 300)
        row[0] += 1
        nt.links.new(uv.outputs["UV"], node.inputs["Vector"])
        images[key] = node
        return node

    def scalar_socket(t):
        """Output socket with one value of the texture (component or grey)."""
        node = image_node(t)
        comp = t.get("component")
        if comp is None:
            sock = node.outputs["Color"]
        elif comp == 3:
            sock = node.outputs["Alpha"]
        else:
            sep = separates.get(t["path"])
            if sep is None:
                sep = nt.nodes.new("ShaderNodeSeparateColor") if hasattr(bpy.types, "ShaderNodeSeparateColor") else nt.nodes.new("ShaderNodeSeparateRGB")
                sep.location = (node.location[0] + 300, node.location[1])
                nt.links.new(node.outputs["Color"], sep.inputs[0])
                separates[t["path"]] = sep
            sock = sep.outputs[comp]
        if t.get("invert"):
            inv = nt.nodes.new("ShaderNodeMath")
            inv.operation = "SUBTRACT"
            inv.inputs[0].default_value = 1.0
            inv.location = (node.location[0] + 450, node.location[1])
            nt.links.new(sock, inv.inputs[1])
            sock = inv.outputs[0]
        return sock

    for t in textures:
        role = t["role"]
        try:
            if role == "base_color":
                nt.links.new(image_node(t).outputs["Color"], bsdf.inputs["Base Color"])
            elif role == "normal":
                node = image_node(t)
                color = node.outputs["Color"]
                if t.get("directx"):
                    # DirectX normal maps: flip green into Blender's OpenGL convention.
                    sep = nt.nodes.new("ShaderNodeSeparateColor") if hasattr(bpy.types, "ShaderNodeSeparateColor") else nt.nodes.new("ShaderNodeSeparateRGB")
                    comb = nt.nodes.new("ShaderNodeCombineColor") if hasattr(bpy.types, "ShaderNodeCombineColor") else nt.nodes.new("ShaderNodeCombineRGB")
                    inv = nt.nodes.new("ShaderNodeMath")
                    inv.operation = "SUBTRACT"
                    inv.inputs[0].default_value = 1.0
                    sep.location = (node.location[0] + 280, node.location[1])
                    inv.location = (node.location[0] + 450, node.location[1])
                    comb.location = (node.location[0] + 620, node.location[1])
                    nt.links.new(color, sep.inputs[0])
                    nt.links.new(sep.outputs[0], comb.inputs[0])
                    nt.links.new(sep.outputs[1], inv.inputs[1])
                    nt.links.new(inv.outputs[0], comb.inputs[1])
                    nt.links.new(sep.outputs[2], comb.inputs[2])
                    color = comb.outputs[0]
                nmap = nt.nodes.new("ShaderNodeNormalMap")
                nmap.location = (250, -350)
                nmap.space = "TANGENT"
                nt.links.new(color, nmap.inputs["Color"])
                nt.links.new(nmap.outputs["Normal"], bsdf.inputs["Normal"])
            elif role == "emission":
                sock = bsdf_input(bsdf, "Emission Color", "Emission")
                nt.links.new(image_node(t).outputs["Color"], sock)
                strength = bsdf_input(bsdf, "Emission Strength")
                if strength is not None:
                    strength.default_value = 1.0
            elif role == "alpha":
                nt.links.new(scalar_socket(t), bsdf.inputs["Alpha"])
                for attr, value in (("surface_render_method", "DITHERED"), ("blend_method", "HASHED")):
                    try:
                        setattr(mat, attr, value)
                    except Exception:
                        pass
            elif role == "ambient_occlusion":
                # The Principled BSDF has no AO input (Blender computes its own); the map is kept
                # in the material so it can be mixed into Base Color or used by game exporters.
                scalar_socket(t)
                image_node(t).label = "AO (not connected) - " + os.path.basename(t["path"])
            elif role == "height":
                disp = nt.nodes.new("ShaderNodeDisplacement")
                disp.location = (500, -500)
                disp.inputs["Midlevel"].default_value = 0.5
                disp.inputs["Scale"].default_value = 0.02
                nt.links.new(scalar_socket(t), disp.inputs["Height"])
                nt.links.new(disp.outputs["Displacement"], out.inputs["Displacement"])
            else:
                names = {
                    "metallic": ("Metallic",),
                    "roughness": ("Roughness",),
                    "specular": ("Specular IOR Level", "Specular"),
                    "coat_weight": ("Coat Weight", "Clearcoat"),
                    "coat_roughness": ("Coat Roughness", "Clearcoat Roughness"),
                    "sheen_weight": ("Sheen Weight", "Sheen"),
                    "anisotropy": ("Anisotropic", "Anisotropy"),
                }.get(role)
                sock = bsdf_input(bsdf, *names) if names else None
                if sock is not None:
                    nt.links.new(scalar_socket(t), sock)
        except Exception as e:
            log("could not connect %s (%s): %s" % (t.get("path"), role, e))


# ----- way back ---------------------------------------------------------------------------------

def stored_manifest(context):
    path = context.scene.get(PROP, "")
    if not path or not os.path.isfile(path):
        return None, path
    try:
        return load_manifest(path), path
    except Exception:
        return None, path


class SUBCRAFT_OT_send_back(bpy.types.Operator):
    """Write the selected object(s), with modifiers (Multires at its highest level), to SubCraft as the bake high poly"""
    bl_idname = "subcraft.send_back"
    bl_label = "Send to SubCraft"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        return any(o.type == "MESH" for o in context.selected_objects) or (context.active_object is not None and context.active_object.type == "MESH")

    def execute(self, context):
        manifest, path = stored_manifest(context)
        if manifest is None:
            self.report({"ERROR"}, "SubCraft exchange file not found (%s). Send the model from SubCraft again." % path)
            return {"CANCELLED"}
        out_dir = manifest["return_dir"]
        os.makedirs(out_dir, exist_ok=True)
        target = os.path.join(out_dir, manifest.get("return_file", "sculpt_high.obj"))
        tmp = target + ".part.obj"

        mode = context.mode
        active = context.view_layer.objects.active
        if active is not None and not active.select_get():
            active.select_set(True)
        if mode != "OBJECT":
            bpy.ops.object.mode_set(mode="OBJECT")
        # Multires: export the highest level whatever the viewport shows.
        restore = []
        for o in context.selected_objects:
            for mod in o.modifiers:
                if mod.type == "MULTIRES":
                    restore.append((mod, mod.levels, mod.render_levels))
                    mod.levels = mod.total_levels
                    mod.render_levels = mod.total_levels
        tris = 0
        try:
            deps = context.evaluated_depsgraph_get()
            for o in context.selected_objects:
                if o.type == "MESH":
                    tris += sum(len(p.vertices) - 2 for p in o.evaluated_get(deps).data.polygons)
            if hasattr(bpy.ops.wm, "obj_export"):
                bpy.ops.wm.obj_export(
                    filepath=tmp, export_selected_objects=True, apply_modifiers=True,
                    export_eval_mode="DAG_EVAL_VIEWPORT", export_uv=False, export_normals=True,
                    export_materials=False, export_triangulated_mesh=True,
                    forward_axis="NEGATIVE_Z", up_axis="Y", global_scale=1.0,
                )
            else:
                bpy.ops.export_scene.obj(filepath=tmp, use_selection=True, use_mesh_modifiers=True, use_uvs=False,
                                         use_materials=False, use_triangles=True, axis_forward="-Z", axis_up="Y")
            os.replace(tmp, target)
        except Exception as e:
            self.report({"ERROR"}, "Could not write the high poly: %s" % e)
            return {"CANCELLED"}
        finally:
            for mod, levels, render in restore:
                mod.levels = levels
                mod.render_levels = render
            if mode == "SCULPT":
                bpy.ops.object.mode_set(mode="SCULPT")
            elif mode.startswith("EDIT"):
                bpy.ops.object.mode_set(mode="EDIT")
        self.report({"INFO"}, "Sent %s triangles to SubCraft" % format(tris, ","))
        log("high poly written to " + target)
        return {"FINISHED"}


class SUBCRAFT_PT_panel(bpy.types.Panel):
    bl_label = "SubCraft"
    bl_idname = "SUBCRAFT_PT_bridge"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "SubCraft"

    def draw(self, context):
        col = self.layout.column()
        col.operator(SUBCRAFT_OT_send_back.bl_idname, icon="EXPORT")
        col.label(text="Selected object -> bake high poly")
        manifest, path = stored_manifest(context)
        if manifest is None:
            col.label(text="Not linked to SubCraft", icon="ERROR")
        else:
            col.label(text="Keep the low poly's position and size", icon="INFO")


CLASSES = (SUBCRAFT_OT_send_back, SUBCRAFT_PT_panel)


def register():
    for cls in CLASSES:
        try:
            bpy.utils.register_class(cls)
        except ValueError:
            bpy.utils.unregister_class(cls)
            bpy.utils.register_class(cls)


# ----- main -----------------------------------------------------------------------------------------

def main():
    path = manifest_path_from_argv()
    register()
    if not path:
        log("no bridge.json given; only the Send to SubCraft panel was added")
        return
    m = load_manifest(path)
    bpy.context.scene[PROP] = os.path.abspath(path)
    clear_default_scene()
    objects = import_obj(m["mesh"])
    if not objects:
        raise RuntimeError("nothing was imported from " + m["mesh"])
    for obj in objects:
        obj.name = m.get("object_name") or obj.name
        if not m.get("include_textures"):
            # Sculpting: one smooth surface, without SubCraft's custom normals.
            bpy.context.view_layer.objects.active = obj
            try:
                bpy.ops.mesh.customdata_custom_splitnormals_clear()
            except Exception:
                pass
            for p in obj.data.polygons:
                p.use_smooth = True
        for entry in m.get("materials", []):
            mat = find_material(obj, entry["name"])
            if mat is None:
                continue
            if m.get("include_textures"):
                build_material(mat, entry.get("textures", []))
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    # Show the textures: Material Preview in every 3D view, framed on the model.
    for window in bpy.context.window_manager.windows:
        for area in window.screen.areas:
            if area.type != "VIEW_3D":
                continue
            for space in area.spaces:
                if space.type == "VIEW_3D" and m.get("include_textures"):
                    space.shading.type = "MATERIAL"
            region = next((r for r in area.regions if r.type == "WINDOW"), None)
            if region is not None:
                try:
                    with bpy.context.temp_override(window=window, area=area, region=region):
                        bpy.ops.view3d.view_selected()
                except Exception:
                    pass
    log("imported %s (%d material(s))" % (m["mesh"], len(m.get("materials", []))))


try:
    main()
except Exception as e:
    import traceback
    traceback.print_exc()
    log("ERROR: %s" % e)
