| Group | Painter key | Painter action | SubCraft action | Status | Note |
|---|---|---|---|---|---|
| General | `Tab` | Hide UI |  | no equivalent | no panel-hiding mode (dock layout is owned by another change) |
| General | `Ctrl+Z` | Undo | Undo | already the same |  |
| General | `Ctrl+Y` | Redo | Redo | already the same |  |
| General | `F8` | Baking mode | Bake | added (key was free) | opens the Bake window |
| General | `F9` | Painting mode |  | no equivalent | no separate painting mode |
| General | `F10` | Rendering (Iray) mode | TogglePathTracing | already the same | path-traced Rendering mode |
| General | `Ctrl+O` | Open project | OpenProject | already the same |  |
| General | `Ctrl+F4` | Close project |  | no equivalent | no close-project command |
| General | `Ctrl+S` | Save project | Save | already the same |  |
| General | `(none)` | Save as |  | no equivalent | Painter has no key; SubCraft keeps Ctrl+Shift+S |
| General | `Ctrl+N` | New project | NewProject | added (key was free) | New project from a model |
| General | `Alt+F4` | Quit application | Quit | added (key was free) | also Ctrl+Q; Windows may close the window itself first |
| General | `Ctrl+Shift+E` | Export textures | Export | already the same |  |
| General | `Ctrl+Shift+B` | Bake mesh maps | Bake | added (key was free) | Ctrl+B kept as well |
| Manipulator | `Q` | Show/hide manipulator |  | no equivalent | no layer transform manipulator |
| Manipulator | `+` | Increase manipulator size |  | no equivalent | no manipulator |
| Manipulator | `-` | Decrease manipulator size |  | no equivalent | no manipulator |
| Manipulator | `T` | Cycle manipulation spaces |  | no equivalent | no manipulator |
| Manipulator | `Shift+V` | Toggle warp edition mode |  | no equivalent | no warp mode |
| Manipulator | `W` | Translate tool |  | no equivalent | no manipulator |
| Manipulator | `E` | Rotate tool |  | no equivalent | no manipulator (E stays Eraser) |
| Manipulator | `R` | Scale tool |  | no equivalent | no manipulator |
| Manipulator | `Shift+W` | Surface tool |  | no equivalent | no equivalent |
| Tools | `L` | Symmetry | ToggleSymmetryX | added (key was free) | toggles X mirror |
| Tools | `Shift+Esc` | Pause engine computation |  | no equivalent | no engine pause |
| Tools | `1` | Paint tool | BrushTool | added (key was free) | B kept as well |
| Tools | `Ctrl+1` | Paint tool + Particles |  | no equivalent | no particle tool (scatter lives in brush settings) |
| Tools | `2` | Eraser tool | EraserTool | added (key was free) | E kept as well |
| Tools | `Ctrl+2` | Eraser tool + Particles |  | no equivalent | no particle tool |
| Tools | `3` | Projection tool |  | no equivalent | no projection tool |
| Tools | `Ctrl+3` | Projection tool + Particles |  | no equivalent | no projection tool |
| Tools | `4` | Polygon Fill | PolygonFillTool | added (key was free) |  |
| Tools | `5` | Smudge tool | SmudgeTool | added (key was free) |  |
| Tools | `6` | Clone tool (relative source) | CloneTool | added (key was free) |  |
| Tools | `Ctrl+6` | Clone tool (absolute source) |  | no equivalent | only one clone mode |
| Tools | `]` | Increase tool size | BrushLarger | already the same |  |
| Tools | `[` | Decrease tool size | BrushSmaller | already the same |  |
| Tools | `X` | Invert grayscale tool |  | no equivalent | no such toggle; in the Painter preset X is unbound (SubCraft's X symmetry moves to L) |
| Tools | `P` | Pick stroke material | EyedropperTool | Painter preset only (conflict) | Eyedropper tool; Pen paths moves to Shift+P in this preset |
| Tools | `D` | Lazy Mouse | ToggleLazyMouse | added (key was free) | toggles a 20 px rope |
| Tools | `Alt+H` | Hide/ignore excluded geometry |  | no equivalent | no equivalent |
| Tools | `N (hold)` | Ignore stencil mask |  | mouse / hold key | already worked |
| Tools | `A` | Resume previous stroke |  | no equivalent | no equivalent |
| Tools | `V (hold) + LMB` | Set Clone tool source |  | mouse / hold key | added; Ctrl+click still works |
| Viewport | `F5` | Camera perspective | PerspectiveCamera | added (key was free) |  |
| Viewport | `F6` | Camera orthographic | OrthographicCamera | added (key was free) |  |
| Viewport | `C` | Display next channel | NextChannel | added (key was free) | cycles the 3D view channels |
| Viewport | `Shift+C` | Display previous channel | PrevChannel | added (key was free) |  |
| Viewport | `M` | Display material | DisplayMaterial | added (key was free) |  |
| Viewport | `B` | Display next mesh map |  | no equivalent | B stays Brush; baked maps are chosen from the view toolbar |
| Viewport | `Shift+B` | Display previous mesh map |  | no equivalent | see B |
| Viewport | `Space` | Toggle animation |  | no equivalent | no animation |
| Viewport | `F` | Center the whole mesh | ResetCamera | added (key was free) | Home kept as well |
| Viewport | `U` | Toggle quick mask edition |  | no equivalent | no quick mask |
| Viewport | `Y` | Clear quick mask |  | no equivalent | no quick mask |
| Viewport | `I` | Invert quick mask |  | no equivalent | no quick mask (I over the model isolates a part) |
| Viewport | `F1` | Viewport layout 3D/2D |  | no equivalent | layout is dock based; arrange tabs instead |
| Viewport | `F2` | Viewport layout 3D only |  | no equivalent | layout is dock based |
| Viewport | `F3` | Viewport layout 2D only |  | no equivalent | layout is dock based |
| Viewport | `F4` | Swap 2D / 3D view |  | no equivalent | layout is dock based |
| Viewport | `Alt+Q` | Texture Set isolate |  | no equivalent | no equivalent key (parts: I / H over the model) |
| Mouse | `LMB` | Use tool / paint |  | mouse / hold key | already worked |
| Mouse | `Shift+LMB` | Draw straight lines |  | mouse / hold key | already worked |
| Mouse | `Ctrl+Shift+LMB` | Draw straight lines with snapping |  | no equivalent | no snapped lines |
| Mouse | `Alt+LMB` | Camera rotate |  | mouse / hold key | already worked (Painter navigation) |
| Mouse | `Alt+Shift+LMB` | Camera snap rotate |  | no equivalent | no snap rotation |
| Mouse | `Alt+MMB` | Camera translate |  | mouse / hold key | already worked (plain MMB too) |
| Mouse | `Ctrl+Alt+LMB` | Camera translate (alternative) |  | mouse / hold key | added |
| Mouse | `Alt+RMB` | Camera zoom |  | mouse / hold key | already worked |
| Mouse | `S (hold) + LMB` | Stencil rotate |  | mouse / hold key | already worked |
| Mouse | `Shift+S + LMB` | Stencil snap rotate |  | no equivalent | no snap |
| Mouse | `S (hold) + MMB` | Stencil translate |  | mouse / hold key | already worked |
| Mouse | `S (hold) + RMB` | Stencil zoom |  | mouse / hold key | already worked |
| Mouse | `Ctrl+RMB drag` | Change tool size / hardness |  | mouse / hold key | added: horizontal = size, vertical = hardness |
| Mouse | `Ctrl+LMB drag` | Change tool flow / rotation |  | mouse / hold key | added: horizontal = flow, vertical = angle (Clone keeps Ctrl+click for its source) |
| Mouse | `Shift+RMB` | Rotate environment |  | mouse / hold key | added: drag left-right turns the environment and its lighting (Shift+MMB turns only the key light) |
| Mouse | `Ctrl+MMB` | Auto-focus (Depth of Field) |  | no equivalent | depth of field is being built on another branch |
| Mouse | `Ctrl+Alt+RMB` | Texture set selection |  | no equivalent | no equivalent |
| Mouse | `RMB` | Context menu |  | mouse / hold key | already worked (part actions) |
| Layers | `Ctrl+C / X / V` | Copy / cut / paste layer |  | mouse / hold key | already worked |
| Layers | `Delete` | Delete layer | DeleteLayer | already the same |  |
| Layers | `Ctrl+D` | Duplicate layer | DuplicateLayer | already the same |  |
| Layers | `Ctrl+G` | Group layer | NewFolder | already the same | creates a folder |
| Layers | `Ctrl+Shift+C` | Copy layer content |  | no equivalent | no content-only clipboard |
| Layers | `Ctrl+Shift+V` | Paste layer content |  | no equivalent | no content-only clipboard |
| Layers | `Alt+LMB on mask` | Display mask in viewport |  | no equivalent | no mask display toggle |
| Layers | `Shift+LMB on mask` | Disable/enable mask |  | no equivalent | mask toggle is in the layer panel |
| Layers | `Ctrl+drag & drop` | Drag material |  | no equivalent | no equivalent |
| Layers | `Shift / Ctrl (drag)` | Manipulator snapping / mirror |  | no equivalent | no manipulator |
