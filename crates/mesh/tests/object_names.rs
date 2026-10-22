//! A real GLB with two instances sharing the same mesh and display name.
use subcraft_mesh::import_mesh;

#[test]
fn fbx_keeps_the_node_name_with_uvs() {
    let path = std::env::temp_dir().join(format!("subcraft-objects-{}.fbx", subcraft_core::ObjectId::new()));
    std::fs::write(&path, r#"; FBX 7.4.0 project file
FBXHeaderExtension: { FBXHeaderVersion: 1003
    FBXVersion: 7400
}
Definitions: { Version: 100
    Count: 2
    ObjectType: "Geometry" { Count: 1 }
    ObjectType: "Model" { Count: 1 }
}
Objects: {
    Geometry: 1001, "Geometry::Triangle", "Mesh" {
        GeometryVersion: 124
        Vertices: *9 { a: 0,0,0,1,0,0,0,1,0 }
        PolygonVertexIndex: *3 { a: 0,1,-3 }
        LayerElementUV: 0 {
            Version: 101
            Name: "UVMap"
            MappingInformationType: "ByPolygonVertex"
            ReferenceInformationType: "Direct"
            UV: *6 { a: 0,0,1,0,0,1 }
        }
        Layer: 0 {
            Version: 100
            LayerElement: { Type: "LayerElementUV"
                TypedIndex: 0
            }
        }
    }
    Model: 1002, "Model::Blade", "Mesh" { Version: 232
        Shading: T
        Culling: "CullingOff"
    }
}
Connections: {
    C: "OO",1001,1002
    C: "OO",1002,0
}
"#).unwrap();
    let mesh = import_mesh(&path).unwrap().mesh;
    assert_eq!(mesh.objects[0].name, "Blade");
    assert_eq!(mesh.tri_object, [Some(0)]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn glb_instances_keep_names_and_distinct_object_uuids() {
    let mut binary = Vec::new();
    for v in [-1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.0] { binary.extend_from_slice(&v.to_le_bytes()); }
    let mut json = br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],"nodes":[{"mesh":0,"name":"Blade","translation":[-2,0,0]},{"mesh":0,"name":"Blade","translation":[2,0,0]}],"meshes":[{"name":"Fallback mesh","primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1}}]}],"buffers":[{"byteLength":60}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":24}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}]}"#.to_vec();
    while !json.len().is_multiple_of(4) { json.push(b' '); }
    let mut glb = Vec::new();
    for word in [0x4654_6c67u32, 2, (12 + 8 + json.len() + 8 + binary.len()) as u32, json.len() as u32, 0x4e4f_534a] { glb.extend_from_slice(&word.to_le_bytes()); }
    glb.extend(json); glb.extend_from_slice(&(binary.len() as u32).to_le_bytes()); glb.extend_from_slice(&0x004e_4942u32.to_le_bytes()); glb.extend(binary);
    let path = std::env::temp_dir().join(format!("subcraft-objects-{}.glb", subcraft_core::ObjectId::new()));
    std::fs::write(&path, glb).unwrap();
    let mesh = import_mesh(&path).unwrap().mesh;
    assert_eq!(mesh.triangle_count(), 2); assert_eq!(mesh.objects.len(), 2);
    assert!(mesh.objects.iter().all(|o| o.name == "Blade"));
    assert_ne!(mesh.objects[0].id, mesh.objects[1].id);
    assert_ne!(mesh.tri_object[0], mesh.tri_object[1]);
    std::fs::remove_file(path).unwrap();
}
