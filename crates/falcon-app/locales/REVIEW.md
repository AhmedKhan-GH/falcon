# Translations to review

Every translation in this folder was written for falcon by a language
model, not by a native speaker, and none was copied from another app. All
of them should be read by a fluent speaker before a release; the list
below says what each file covers and where a reviewer should look first.

When a string is reviewed, nothing needs to change here unless a whole
language has been checked: then move it to "Reviewed" with the date and
the reviewer.

## Conventions

- German addresses the user as *Sie*; Spanish uses neutral imperatives
  and infinitives for commands (*Mover a la papelera*), no *tú*/*usted*.
- Japanese and Korean use polite forms in sentences (です/ます, 합니다/하세요)
  and nouns or plain verbs in menu items; Japanese uses full-width
  punctuation (、。（）「」) and the menu ellipsis `…`.
- Chinese (both files) uses full-width punctuation (，。：（）), a space
  between Chinese and a Latin word (在 Finder 中显示, 关于 Falcon, PNG 文件)
  and no space between Chinese and a number or a placeholder
  (已加入{n}个项目, 移到{folder}). Earlier files mixed both; they were
  made to follow this rule mechanically, so a reader should look for
  spacing that reads oddly.
- Placeholders such as `{n}`, `{name}` and `{folder}` are kept exactly;
  `…` at the end of a menu item (it opens a dialog) is kept.
- Product and file names stay as they are: Falcon, Eagle, Finder,
  Quick Look, .eaglepack, CSV, Base64, IPTC, XMP, BPM, URL.

## Glossary

The core words, so every file uses one term for one thing.

| English | de | es | ja | ko | ru | zh_CN | zh_TW |
|---|---|---|---|---|---|---|---|
| Library | Mediathek | Biblioteca | ライブラリ | 라이브러리 | Библиотека | 资源库 | 資料庫 |
| Item | Objekt | Elemento | アイテム | 항목 | Элемент | 项目 | 項目 |
| Folder | Ordner | Carpeta | フォルダ | 폴더 | Папка | 文件夹 | 資料夾 |
| Subfolder | Unterordner | Subcarpeta | サブフォルダ | 하위 폴더 | Вложенная папка | 子文件夹 | 子資料夾 |
| Smart Folder | Intelligenter Ordner | Carpeta inteligente | スマートフォルダ | 스마트 폴더 | Смарт-папка | 智能文件夹 | 智慧型資料夾 |
| Tag | Tag | Etiqueta | タグ | 태그 | Тег | 标签 | 標籤 |
| Tag group | Tag-Gruppe | Grupo de etiquetas | タググループ | 태그 그룹 | Группа тегов | 标签组 | 標籤群組 |
| Auto-tags | Auto-Tags | Etiquetas automáticas | 自動タグ | 자동 태그 | Автотеги | 自动标签 | 自動標籤 |
| Uncategorized | Ohne Ordner | Sin carpeta | 未分類 | 미분류 | Без папки | 未分类 | 未分類 |
| Untagged | Ohne Tags | Sin etiquetas | タグなし | 태그 없음 | Без тегов | 无标签 | 無標籤 |
| Recently Used | Zuletzt verwendet | Usados recientemente | 最近使った項目 | 최근 사용 | Недавние | 最近使用 | 最近使用 |
| Random | Zufällig | Aleatorio | ランダム | 무작위 | Случайные | 随机 | 隨機 |
| All Items | Alle Objekte | Todos los elementos | すべてのアイテム | 모든 항목 | Все элементы | 全部项目 | 所有項目 |
| All Tags | Alle Tags | Todas las etiquetas | すべてのタグ | 모든 태그 | Все теги | 全部标签 | 所有標籤 |
| Quick Access | Schnellzugriff | Acceso rápido | クイックアクセス | 빠른 접근 | Быстрый доступ | 快速访问 | 快速存取 |
| Trash | Papierkorb | Papelera | ゴミ箱 | 휴지통 | Корзина | 废纸篓 | 垃圾桶 |
| Sidebar | Seitenleiste | Barra lateral | サイドバー | 사이드바 | Боковая панель | 边栏 | 側邊欄 |
| Inspector | Informationen | Inspector | インスペクタ | 속성 패널 | Инспектор | 检查器 | 檢閱器 |
| Viewer | Betrachter | Visor | ビューア | 뷰어 | Просмотрщик | 查看器 | 檢視器 |
| Filter / Filter bar | Filter / Filterleiste | Filtro / Barra de filtros | フィルタ / フィルタバー | 필터 / 필터 막대 | Фильтр / Панель фильтров | 筛选 / 筛选栏 | 篩選 / 篩選列 |
| Search | Suchen | Buscar | 検索 | 검색 | Поиск | 搜索 | 搜尋 |
| Preferences | Einstellungen | Preferencias | 環境設定 | 환경설정 | Настройки | 偏好设置 | 偏好設定 |
| Import / Export | Importieren / Exportieren | Importar / Exportar | 読み込む / 書き出す | 가져오기 / 내보내기 | Импорт / Экспорт | 导入 / 导出 | 匯入 / 匯出 |
| Thumbnail | Miniatur | Miniatura | サムネール | 썸네일 | Миниатюра | 缩略图 | 縮圖 |
| Rating | Bewertung | Valoración | 評価 | 별점 | Рейтинг | 评分 | 評分 |
| Comment (on an image) | Kommentar | Comentario | コメント | 코멘트 | Комментарий | 批注 | 註解 |
| Notes | Notizen | Notas | メモ | 메모 | Заметки | 备注 | 備註 |
| Source URL | Quell-URL | URL de origen | ソースURL | 출처 URL | URL источника | 来源网址 | 來源網址 |
| Duplicate (verb) | Duplizieren | Duplicar | 複製 | 복제 | Дублировать | 创建副本 | 複製 |
| Duplicates (found) | Duplikate | Duplicados | 重複 | 중복 항목 | Дубликаты | 重复项 | 重複項目 |
| Collect window | Sammelfenster | Ventana de captura | 収集ウインドウ | 수집 창 | Окно сбора | 收集窗口 | 收集視窗 |
| Waterfall / Justified / Grid / List | Wasserfall / Blocksatz / Raster / Liste | Cascada / Justificado / Cuadrícula / Lista | ウォーターフォール / 両端揃え / グリッド / リスト | 워터폴 / 양쪽 맞춤 / 격자 / 목록 | Каскад / По ширине / Сетка / Список | 瀑布流 / 自适应 / 网格 / 列表 | 瀑布流 / 自適應 / 網格 / 列表 |
| Rename | Umbenennen | Renombrar | 名前を変更 | 이름 변경 | Переименовать | 重命名 | 重新命名 |
| Move to Trash | In den Papierkorb legen | Mover a la papelera | ゴミ箱に入れる | 휴지통으로 이동 | Переместить в корзину | 移到废纸篓 | 丟到垃圾桶 |
| Restore | Wiederherstellen | Restaurar | 元に戻す | 복원 | Восстановить | 恢复 | 回復 |
| Cancel / OK | Abbrechen / OK | Cancelar / Aceptar | キャンセル / OK | 취소 / 확인 | Отмена / ОК | 取消 / 好 | 取消 / 好 |
| Undo / Redo | Widerrufen / Wiederholen | Deshacer / Rehacer | 取り消す / やり直す | 실행 취소 / 실행 복귀 | Отменить / Повторить | 撤销 / 重做 | 還原 / 重做 |
| Shortcut | Tastenkürzel | Atajo | ショートカット | 단축키 | Сочетание клавиш | 快捷键 | 快速鍵 |
| Show in Finder | Im Finder zeigen | Mostrar en Finder | Finderで表示 | Finder에서 보기 | Показать в Finder | 在访达中显示 | 在 Finder 中顯示 |
| Presentation mode | Präsentationsmodus | Modo presentación | プレゼンテーションモード | 프레젠테이션 모드 | Режим презентации | 演示模式 | 簡報模式 |

## Files

| File | Keys | State |
|---|---|---|
| de.json | all | machine-written, unreviewed |
| es.json | all | machine-written, unreviewed |
| ja.json | all | machine-written, unreviewed |
| ko.json | all | machine-written, unreviewed |
| ru.json | all | machine-written, unreviewed |
| zh_CN.json | all | machine-written, unreviewed |
| zh_TW.json | all | machine-written, unreviewed |

## Look at first

- Short labels that are ambiguous out of context: *Zoom* (the window
  command), *Grid* (layout), *Size* (file size), *Open* (verb), *None*
  (no binding, no rating, no text), *Fixed* (the shortcut heading: keys
  that can't be changed), *Clear*, *Remove*, *Merge*, *Set*,
  *Fit*/*Fill*, *Starred*, *Inside* (a smart folder's parent row),
  *Has*/*None* (text filters), *Min*/*Max*, *Source*/*Reason*
  (failed-import columns), *recent*. Where one English word needed two
  translations it now has a context key: `tags::Uncategorized` and
  `tags::All` (tags, not items or folders), `match::Any`/`match::All`
  (the filters' match picker), `size::Custom` and `shape::Custom`
  (grammatical gender in es and ru). Check
  whether any other short word still reads wrong where it is shown.
- Sentences split across widgets, whose word order can't change: the
  smart-folder rule row ("Items" / "must match" / "all" / "any" / "of
  these rules", and the method words *is*, *on*, *and*, *days*,
  *between*), and the radio choices that finish the line above them in
  Preferences ("The wheel over an image in the viewer" + "Scrolls it").
- Fragments followed by a folder link: "Added {n} items to", "Moved {n}
  items to", "Added {n} to"; the translations end with a connector.
- Counts placed inside other sentences ("{folders} and {items} from
  {path} will be added…", "Added {items}, {folders} and {others} from
  {name}.", "Importing {images} and {folders} from Inboard", the
  duplicate review footer): Russian case after the count and Japanese
  and Korean particles after a placeholder (와(과), 을(를)) need a
  speaker's eye.
- Russian plural forms (`one`/`few`/`many`) of every count, and the
  forms used to avoid declining a name ("в папки: {count}", "в команде
  «{command}»").
- macOS terms chosen per language: de *Ablage*, *Darstellung*,
  *Einsetzen*, *Im Dock ablegen*; ja *しまう*; zh *拷贝*/*拷貝* (Copy)
  beside *创建副本*/*複製* (Duplicate); es *Edición*, *Visualización*.
- Long sentences in Preferences and the storage and iCloud warnings,
  which must fit a 520-point-wide pane, especially in de, ru, ja and ko.
- The screenshot, menu bar icon, global key and notification strings
  (Preferences ▸ Screenshot, the Global shortcut group, the menu bar
  icon's menu, the collect window's waiting screenshot, "Screenshot
  added", "Import finished", "Export finished"), machine-written in one
  pass; "Screenshot" is also the English tag name, which stays English.
- The New… menu, the import queue's status, the broken-thumbnail hints and
  the new shortcut labels (Add, New…, Other Documents, "{app} Document",
  Text Document, "Not available: falcon has no template file…", Processing
  / Paused ({n}), Click to pause / resume, "Paused at {done} of {total}",
  the three thumbnail hints, "(second key)", "Set the first selected item
  as the folder cover"), machine-written in one pass; "{app}" stands for
  an app name (Word, Keynote, …) and is not translated.
- "{chord} is taken by another app", the note on a global key's row
  when another app holds its chord (machine-written).
- The no-library screen: "Open Library" and "A library is a folder that holds
  your files and their tags." (machine-written).
- The quit warning ("Quit While Tasks Run", "Tasks are still running.
  Quitting stops them. Work already done is kept.", "Stop Tasks and
  Quit"), machine-written with the terms of "Close Library While Tasks
  Run".
- "Stopped after exporting {done} of {n} items", the toast of an
  Export to Computer stopped from its progress modal (machine-written).

- The viewers, sidebar and context menus (the follow-up pass),
  machine-written: the 3D viewer's terms (Perspective, Orthographic, Fixed
  Up Vector, Free Orbit, Shading, Vertices, Surface), the PDF viewer's
  (Wrapped Scrolling, Odd/Even Spreads, Match Diacritics, Fast web view,
  "{current} of {total}"), the font viewer's script groups (Hanzi, Kanji,
  Hangul) and Activate (= install the font), the folder move sentences
  ("Move … and {n} other folders above/into/below") with their short
  Above, Into and Below buttons, Star/Unstar (verbs: mark a tag as a
  favourite), the folder rename templates "Folder %N" and "Smart Folder
  %N" (translated, since they fill the field the user edits), the
  Developer and Auto-Import panes' long explanations, and the Russian
  "Label: N" phrasing used where agreement with a count is awkward.
- Machine-written: "Retry this file" (the failed-imports list's per-row retry button).

- "Enter the password of “{name}” to put the items in it", the prompt when
  items are dropped onto a locked folder (machine-written).
- "Open the folder picker", the Preferences ▸ Shortcuts row of the bare F
  (machine-written).
- "Export as {format}…", the 3D viewer's export menu, replacing the two
  fixed OBJ and STL entries; {format} is GLB, glTF, OFF, OBJ, PLY or STL and is
  not translated (machine-written).
- "Current List", "Selected" and "Settings…", the New… menu's Find Duplicates scopes and Auto-Import entry (machine-written).
- "Untitled" and "tag_group::Untitled", the default names of a new folder, smart folder group and tag group (machine-written).
- Preferences ▸ Notification ▸ Sound effects: the block title, "Play sound effects", the three "When …" switches and the note under them, plus the shortened pop-ups note (its last sentence dropped), machine-written. "Sound effects" follows each system's own term (Toneffekte, 効果音, 효과음, 音效).
- The IFC rows of the inspector ("3D Preview", "IFC: no tessellated geometry", "Not Read", "Products Shown", "Products Skipped", "Reading…", "Schema", "The file could not be read") (machine-written).
- The Actions panel, editor and menu strings (step names, the empty and error states, "Run on {n} items", "{name} changed {n} items", the shortcut hints, "This action has steps Falcon can't run"), machine-written in one pass; "action" is the existing term for "Actions" in each file.
- "Neither macOS nor FFmpeg could play this video…" and "macOS can't play .{ext} videos, and the FFmpeg libraries Falcon plays them with weren't found…", the video viewer's can't-play lines; FFmpeg is a product name and is not translated (machine-written).
- "This web archive couldn't be read. Open it in its own app to browse it.", the web viewer's note when an MHTML archive can't be unpacked (machine-written).

## Reviewed

Nothing yet.
