image

local animation that dont need relayout

checkbox

out.meshes
.arena
.vertices
.extend_from_slice(&cmds.shape_payloads.meshes.vertices[v_start..v_end]);
let phys_i_start = out.meshes.arena.indices.len() as u32;
out.meshes
.arena
.indices
.extend_from_slice(&cmds.shape_payloads.meshes.indices[i_start..i_end]);

out_meshes.vertices.extend(src_verts.iter().map(|v| {
let pos = v.pos + origin;
min = min.min(pos);
max = max.max(pos);
