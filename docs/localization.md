# Interface languages

SubCraft's interface can be shown in **English** (default), **繁體中文** (Traditional Chinese) and
**한국어** (Korean). Pick the language in **Edit / File → Preferences → Interface → Language**; it
applies immediately (no restart) and is saved with the other preferences (`language` in the
settings file; older settings files without it stay English).

> **Both translations need a review by a native speaker.** They were written by hand using the
> usual terminology below, not by a translator. Corrections are welcome as edits to
> `crates/ui/src/i18n/ko.rs` and `crates/ui/src/i18n/zh_hant.rs`.

## How it works (for contributors)

* `crates/ui/src/i18n.rs`: `tr("English text")` returns the text in the current language, or the
  English text itself when there is no translation, so strings can be wrapped one at a time.
  `trf("Baked {} map(s)", &[&n])` fills `{}` placeholders in order.
* Wrap a string where it is drawn: `ui.button(tr("Import"))`, `item(ui, icon, tr("Save"), "Ctrl+S")`.
  Do not wrap strings that are compared, stored, used as ids or written to files.
* Add the English text and both translations to the two tables. Unit tests fail when a
  translation key is not used by any `tr("...")` call (a typo), when a wrapped text lacks a
  translation, or when `{}` / `{n}` placeholders differ from the English text.
* Fonts: SubCraft does not bundle a CJK font. When a CJK language is chosen it loads one from the
  system (Windows: Malgun Gothic / Microsoft JhengHei; macOS: Apple SD Gothic Neo / PingFang;
  Linux: Noto Sans CJK, WenQuanYi Zen Hei, Nanum Gothic) as a fallback behind Inter. Set
  `SUBCRAFT_CJK_FONT=/path/to/font.ttf` (or `.ttc`) to use another; the Preferences window warns
  when none is found.

## Terminology

| English | 한국어 | 繁體中文 |
|---|---|---|
| Layer | 레이어 | 圖層 |
| Mask | 마스크 | 遮罩 |
| Brush | 브러시 | 筆刷 |
| Opacity | 불투명도 | 不透明度 |
| Export / Import | 내보내기 / 가져오기 | 匯出 / 匯入 |
| Bake | 베이크 | 烘焙 |
| Texture | 텍스처 | 貼圖 |
| Material | 머티리얼 | 材質 |
| Mesh | 메시 | 網格 |
| Channel | 채널 | 通道 |
| Folder | 폴더 | 資料夾 |
| Preferences | 환경 설정 | 偏好設定 |
| Properties | 속성 | 屬性 |
| History | 히스토리 | 歷程記錄 |
| Generator | 제너레이터 | 產生器 |
| Smart material / mask | 스마트 머티리얼 / 마스크 | 智慧材質 / 遮罩 |
| Curvature | 곡률 | 曲率 |
| Ambient occlusion | 앰비언트 오클루전 | 環境遮蔽 |
| Project | 프로젝트 | 專案 |
| Resolution | 해상도 | 解析度 |
| Invert | 반전 | 反相 |
| Rotation / Scale / Offset | 회전 / 크기 / 오프셋 | 旋轉 / 縮放 / 位移 |

Korean follows the words used by Photoshop, Blender and Windows (레이어, 마스크, 브러시, 불투명도,
내보내기, 가져오기, 환경 설정). Traditional Chinese follows Taiwan usage (圖層, 遮罩, 筆刷, 不透明度,
匯出, 匯入, 偏好設定).
