# CLIENT_REFACTOR — design-fidelity pass over the Syncra desktop client

> **FOR THE IMPLEMENTING AGENT — HOW TO USE THIS FILE**
>
> This file is the live progress record. Every task below is a checkbox.
> **When a task is finished, mark it with a capital `X`: change `- [ ]` to `- [X]`.**
> Lowercase `x`, `~`, strikethrough or deletion are not accepted — only `- [X]`.
> Do not tick a box until the change is written, `typecheck` / `lint` / `test` are clean for it,
> and it has been eyeballed against the mockup. If a task turns out to be wrong or impossible,
> leave the box empty and add a `> NOTE:` line under it explaining why.

---

## Context

The desktop UI was built in F13 against `design/Syncra Прототип.dc.html`, but a hands-on review
found ~24 places where the running app has drifted from the mockups. Some are cosmetic (a CSS
token that is referenced but never defined, so a popover renders with square corners), some are
structural (the section picker is a native `<select>` whose dropdown cannot be themed), and some
are semantic (footer copy that says something different from the design).

Several of F13's **deliberate** deviations, recorded in `TASKS.md` under "Намеренные отступления",
are being reversed on the user's instruction: the sort control must exist and must actually sort,
and the record card must show a live TOTP code.

**Source of truth: the `design/` folder** — primarily `Syncra Прототип.dc.html`, with
`Syncra Design System.dc.html`, `Syncra Data and Settings.dc.html` and `Syncra Trust Flows.dc.html`
as supporting references. Line numbers cited below refer to those files.

Two areas were explicitly declared "better than the mockup, do not touch": the sections management
plate (user item 17) and the password/security settings pane (user item 20).

### Decisions already taken — do not re-litigate

| Question | Decision |
|---|---|
| Live TOTP code | Add `get_totp_code` to the IPC contract + mock. **No crypto in the frontend** (Закон №1). |
| Theme / accent settings | New "Оформление" tab in `/settings` with theme *and* the 4 accents from the mockup (via `data-accent`). Remove the titlebar toggle. |
| Password reveal in master-password inputs | Remove it — the mockups have no reveal on password *inputs*, only on stored secrets. |
| Sort options | Three: Недавние / По алфавиту / Старые пароли. |

### Ground rules

1. No new dependencies. No crypto in the frontend. Secrets never enter Pinia or `localStorage`.
2. Every hardcoded px/colour you touch must come from `src/assets/tokens.css`. If a token is
   missing, add it there rather than inlining the value.
3. Definition of Done per `CLAUDE.md`: `npm run typecheck`, `npm run lint`, `npm run test` all
   clean, plus a manual pass against the mock core.
4. **There is uncommitted work** on `ConflictDialog.vue`, `PasswordGenerator.vue`,
   `RecordForm.vue` and four spec files. Read the working tree, not `HEAD`.
5. Work the phases in order — phases 2-5 depend on the primitives built in phase 1.

---

## Phase 1 — Foundations

These unblock everything else. Do them first.

- [X] **1.1 Fix and extend design tokens** — `src/assets/tokens.css`
  - `--sy-radius-md` is **referenced but never defined** (`RecordCard.vue:659`), so the "···"
    popover renders at `border-radius: 0`. **This is the root cause of user item 10.** Do not add
    an `md` rung — the mockup's popover is 10px, which is `--sy-radius`. Fix the call site, then
    grep the whole `src/` tree for any other undefined `--sy-*` reference.
  - Add `--sy-control-height-md: 34px`. The mockup uses 34px consistently for pane-header actions,
    and it is not on the current 30/36/42 ladder.
  - Add the four accent palettes from `Прототип:2595-2600` as
    `:root[data-accent='cyan'|'amber'|'indigo']` blocks overriding `--sy-accent`,
    `--sy-accent-quiet`, `--sy-accent-border`, `--sy-accent-fg`. `mint` (hue 168) stays the
    default with no attribute. Required by phase 5.1.
  - Add the small type sizes components currently hardcode (`10px`, `10.5px`, `11.5px`, `13px`,
    `14.5px`) and the `5px` / `7px` radii used by chips and swatches.

- [X] **1.2 `SyButton` — icon-only variant**
  Four icon buttons are hand-rolled today (`.card__menu-button`, `.record-list__new`,
  `.record-list__copy`, `.form__url-remove`), which is why user item 3's height mismatch exists at
  all. Add an `icon?: boolean` prop producing a square button whose side equals the chosen size.
  Then rebuild the record-card header: **«Изменить»** and **«···»** both at 34px,
  `--sy-radius-sm`, `1px solid var(--sy-border-strong)`, `background var(--sy-surface)`, hover
  `--sy-surface-2` (`Прототип:1520-1522`). Today `SyButton size="sm"` is 30px while
  `.card__menu-button` is a hardcoded 34px.

- [X] **1.3 `SySelect` — replace the native `<select>` with the mockup's custom dropdown**
  > NOTE: the trigger is specified at 38px, but `SyInput` rendered its box at 42px
  > (`--sy-control-height-lg`), and the two stand side by side in `.form__pair`. The mockup puts
  > form fields at 38px too (`Прототип:1721`, and task 4.5 assumes it), so the field height was
  > split out of the button ladder into `--sy-control-height-field: 38px` and `SyInput` now uses
  > it. `SyButton size="lg"` is untouched at 42px.
  A native `<select>` renders OS chrome that cannot be themed — **this is user item 12.** Rewrite
  `SySelect.vue` keeping its public API (`modelValue`, `label`, `options`, `hint`, `disabled`) and
  adding an optional per-option `dot` colour and a `#footer` slot. Spec from `Прототип:1741-1769`:
  - **Trigger**: `width:100%`, height 38px, `--sy-radius-sm`, `1px solid var(--sy-border)`,
    `background var(--sy-surface)`, padding `0 12px`, hover `border-color: var(--sy-border-strong)`.
    Left: 7×7 `border-radius:2px` colour dot + 14px label (ellipsised). Right: CSS chevron —
    7×7 box, right+bottom borders `1.5px solid var(--sy-text-3)`, `rotate(45deg) translateY(-2px)`.
  - **Panel**: `position:absolute; top:42px; left:0; right:0; z-index:20; max-height:200px;
    overflow:auto`, `1px solid var(--sy-border-strong)`, `border-radius:9px`,
    `background var(--sy-surface)`, `box-shadow: var(--sy-shadow-window-2)`, `padding:5px`,
    item gap 2px, `animation: sy-in 0.14s ease-out`.
  - **Item**: full-width, 34px, `border-radius:7px`, `border:none`, padding `0 10px`, gap 9px,
    dot + 13.5px label, `text-align:left`. Selected → `background var(--sy-accent-quiet)`, label
    `var(--sy-accent)` weight 600, **no hover**. Idle → transparent, label `var(--sy-text-2)`,
    hover `background var(--sy-surface-2)`.
  - **Footer slot**: 1px `--sy-border` rule (`margin: 4px 6px`), then a 32px borderless accent
    link-button, left-aligned, hover `--sy-surface-2`.
  - **Keyboard / a11y**: `role="combobox"` + `role="listbox"`, ↑↓/Home/End/Enter/Escape, close on
    outside mousedown. Reuse the popover open/close pattern already in `RecordCard.vue:126-155`.

- [ ] **1.4 `SyField` — a field with a trailing action**
  The magic number `margin-top: 22px` appears three times to nudge a sibling button past the label
  (`RecordForm.vue:738-751`, `RecordForm.vue:764`, `SectionEditor.vue:141-146`). Introduce a small
  `SyField` wrapper (label + control slot + `#action` slot) that aligns by grid instead of by a
  hardcoded offset, and adopt it in all three places.

- [ ] **1.5 `SyModal` — decouple the warning strip's colour, add a banded layout**
  - The mockup's delete dialog is a **danger-bordered card containing an amber warn strip**
    (`Прототип:2262-2283`): two risk levels, two tokens. Today `.sy-modal__warning--danger`
    inherits from `tone`, so the caveat turns red. Add `warningTone?: 'warning' | 'danger'`
    defaulting to `tone`. **Part of user item 9.**
  - Add `banded?: boolean`: header block with `border-bottom`, scrolling body, footer bar on
    `var(--sy-bg-1)` with `border-top` — the shape of the 560/860px dialogs
    (`Прототип:2298-2376`, `2378-2438`). Required by phases 3 and 4.7.
  - While in the file: add a focus trap and focus restore on close (both currently absent).

---

## Phase 2 — Vault list and record card

- [ ] **2.1 Search field spans the column** *(user item 1)*
  `.record-list__search` (`RecordList.vue:366`) has no `flex`, so it shrinks to the input's
  intrinsic width. Add `flex: 1; min-width: 0;`. Note that in the mockup (`Прототип:1416-1424`)
  the search sits at `flex:1` in a `gap:8px` row beside the 34×34 accent "+" button — so "full
  column width" means the *row* is full width and the search takes what the button does not
  (≈306px of the 376px column). That layout is already correct; only the grow is missing.

- [ ] **2.2 Sort control replaces the Ctrl+K chip** *(user item 2)*
  Remove `.record-list__hotkey` (`RecordList.vue:164`) — the string "Ctrl" appears nowhere in the
  mockups. **Keep the keydown handler** (`RecordList.vue:113-127`); it is a good affordance, just
  not a visible chip. In its place put the mockup's control (`Прототип:1425-1427`): right side of
  the counter row, 12px `var(--sy-text-2)` label + a 6×6 CSS chevron, implemented as an inline
  (borderless) `SySelect` from phase 1.3.
  Sort state lives in `useRecordsStore` as an `order` ref and is consumed by
  `useRecordList` / `groupRecords` (`useRecordList.ts:150-169`):

  | Label | Key |
  |---|---|
  | Недавние | `updated_at` descending |
  | По алфавиту | `service_name`, `localeCompare(…, 'ru')` — today's behaviour |
  | Старые пароли | `password_updated_at` ascending |

  Group ordering follows the chosen key; in-group ordering stays `account_label` → `login`.

- [ ] **2.3 Record card header buttons are the same height** *(user item 3)*
  Delivered by 1.2. Additionally set `.card__head-actions` to `align-items: center` —
  `.card__head` is `align-items: flex-start`, which would still misalign two equal-height buttons
  against the 46px avatar block.

- [ ] **2.4 Remove the «Версия» block** *(user item 4)*
  Delete `RecordCard.vue:384-391`. There is no version block anywhere in the desktop mockup; the
  only version-adjacent surfaces are the conflict banner and the mono footer line. Keep
  `record.version` in the store — it is contract data, simply not displayed.

- [ ] **2.5 Copy button labels match the mockup** *(user item 5)*
  The mockup uses two labels for the password (`Прототип:1623-1639`):
  - hidden state → **«Копировать пароль»**, accent-filled (`background var(--sy-accent)`,
    `color var(--sy-accent-fg)`, weight 600)
  - revealed state → **«Копировать»** beside **«Скрыть»**, both 32px, `border-radius:7px`,
    `1px solid var(--sy-accent-border)`, transparent, `color var(--sy-accent)`, 12.5px

  `SySecretField.vue:98` hardcodes `Копировать` for every secret. Add a `copyLabel?: string` prop
  defaulting to `Копировать`, switching to the field-specific hidden-state label when closed; the
  card passes «Копировать пароль». Align the copied-state label with the mockup's `copyLabel`
  (`Прототип:2904`): `Скопировано · {N} с`. Metadata rows keep «Скопировать адрес» /
  «Скопировать логин» — they are icon-only with the text in `title` (`Прототип:1568,1590`).

- [ ] **2.6 Live TOTP code with 3+3 grouping and a countdown ring** *(user item 6)*
  **Contract change — record it in `TASKS.md` and get the backend agent's confirmation.**
  - `src/core/contract.ts`: add `get_totp_code` → request `{ record_id }`, response
    `{ code: string; seconds_left: number; period_s: number }`. `code` is the **generated code**,
    never the secret. Add the snake_case wire name to `COMMAND_NAMES`.
  - `src/core/mock/`: generate a stable-per-record 6-digit code and a real countdown off
    `Date.now()`, so the ring animates in dev.
  - `useRecordSecrets` (or a sibling `useTotpCode`) fetches on reveal, re-fetches when the window
    expires, and drops the value on hide / record change / `locked` — the same discipline as
    secrets. **The code is a secret: it must never land in a store.**
  - Card rendering, verbatim from `Прототип:1652-1673`. The TOTP cell is the fixed **280px** left
    column of a `grid-template-columns: 280px 1fr; gap:16px` pair shared with Заметки:
    - label «Код TOTP» — mono 10.5px, `letter-spacing:0.1em`, uppercase, `--sy-text-3`
    - **revealed**: 46px row, `--sy-radius-sm`, `1px solid var(--sy-accent-border)`,
      `background var(--sy-accent-quiet)`, padding `0 12px 0 14px`, the whole row is the toggle.
      Code in **mono 17px, `letter-spacing: 0.14em`**, grouped **3 + 3 with a single space**
      (`418 902`). Right cluster gap 9px: mono 11px `--sy-text-2` «{n} с» + an 18×18 ring
      (`border:2px solid var(--sy-accent-border)`, top and right in `--sy-accent`) driven by
      `seconds_left / period_s`.
    - **hidden**: mono 16px, `letter-spacing:0.22em`, `--sy-text-2`, showing `••• •••`
      (**3+3, not the current 10-dot mask**) + a 32px «Показать» chip.
    - **empty**: `1px dashed var(--sy-border-strong)`, no fill, «Не подключён».
  - The raw TOTP **secret** is no longer surfaced in the card — the mockup never shows it. It
    stays editable in the form.

- [ ] **2.7 Notes field matches the mockup** *(user item 7)*
  `Прототип:1675-1692`. The hidden state is **not** a masked value with buttons — it is
  **two skeleton bars** (8px tall, `border-radius:3px`, `background var(--sy-surface-2)`, widths
  70% and 45%, gap 6px) plus a single «Показать» chip, and the whole box is the toggle.
  Revealed: `1px solid var(--sy-accent-border)` / `background var(--sy-accent-quiet)`,
  `min-height:46px`, padding `12px 14px`, 13.5px, `line-height:1.5`. Empty: dashed, «Заметок нет».
  **No copy button and no second button** — remove them for notes. Add a skeleton presentation
  mode to `SySecretField` rather than forking the component.

- [ ] **2.8 Card footer copy** *(user item 8)*
  `.card__foot-note` (`RecordCard.vue:458-466`) currently reads "Хранится на этом устройстве ·
  копии есть на сопряжённых устройствах". The mockup's `storedLine` (`Прототип:2896`) is
  **«хранится на N устройствах · последняя версия отсюда»** — lowercase, mono 10.5px, stating a
  *count* and *provenance* rather than a reassurance. Rewrite against the device count from
  `useDevicesStore`, keeping the two existing conditional branches (local section / pending
  change) but re-worded in the mockup's register. Keep «Удалить запись» on the right
  (`Прототип:1698`): 32px, `--sy-radius-sm`, `1px solid var(--sy-danger)`,
  `background var(--sy-danger-quiet)`, 12.5px/600, hover fills solid with `--sy-danger-fg`.

- [ ] **2.9 Delete dialog — order, overflow, colours, type** *(user item 9)*
  Four separate defects in `RecordCard.vue:470-497` against `Прототип:2262-2283`:
  1. **Order.** The mockup is title → mono meta → body → warn strip → footer. `SyModal` renders
     the `warning` prop *above* the body, so the strip currently precedes the meta line. Move the
     caveat into the default slot after the body paragraph.
  2. **Overflow — "текст выходит за рамки формы".** The hint is passed inside `#actions` as a
     `flex: 1` span, but `.sy-modal__buttons` is `flex: none`, so the row grows past the dialog.
     **Use `SyModal`'s dedicated `#note` slot** (`SyModal.vue:102-107`), which is exactly this
     pattern and is already styled at 12px `--sy-text-3`.
  3. **Mismatched risk colours.** Card border = `--sy-danger`; the TOTP caveat strip =
     `--sy-warn` / `--sy-warn-quiet` with a 6px warn dot. Uses `warningTone` from phase 1.5.
  4. **Typography and width.** Add `size="confirm"` (480px — `SyModal` documents that size for
     exactly this case; today it falls back to the 460px default). Meta line mono **11px**
     `--sy-text-3`; body 13.5px `--sy-text-2`, `line-height:1.55`; title 20px/600.

  Run the same `size="confirm"` + `#note` audit over `SectionsView.vue:381-397`. Note that the
  mockup deliberately makes the **section**-delete confirm button *neutral*, not danger
  (`Прототип:2285-2296`) — verify the current implementation matches.

- [ ] **2.10 "···" popover has round corners** *(user item 10)*
  Root cause is 1.1. After the token fix the popover is `--sy-radius` (10px), border
  `--sy-border-strong`, shadow `--sy-shadow-window-2`, width 278px, padding 6px — matching
  `Прототип:1524`. The content stays the verbatim "Ещё думаем" placeholder.

---

## Phase 3 — Version / conflict dialog

- [ ] **3.1 Rebuild `ConflictDialog.vue` against the mockup** *(user item 11)*
  The "version picker" is `ConflictDialog.vue`. There is **no separate version-history dialog
  anywhere in the designs** — history appears only in the "···" popover as an unbuilt idea. Do not
  invent one. Restructure against `Прототип:2378-2438`:
  - **Banded shell** (phase 1.5), 860px: header `padding:20px 24px` with
    `border-bottom:1px solid var(--sy-border)`, holding the 20px/600 title
    «Две версии одной записи» and a 13px `--sy-text-2` lead. Today the title and lead are a flat
    `<h2>` + `<p>` with no band — **this is the main structural gap.**
  - Body: `padding:20px 24px; display:grid; grid-template-columns:1fr 1fr; gap:16px`.
  - Footer bar: `padding:16px 24px; border-top:1px solid var(--sy-border);
    background: var(--sy-bg-1)`, mono 10.5px diff line on the left, «Решить позже» + primary right.
  - **Version card**: `border-radius:11px`, `padding:16px`, gap 12px. Selected →
    `1px solid var(--sy-accent)`, `background var(--sy-accent-quiet)`,
    `box-shadow: 0 0 0 3px var(--sy-accent-quiet)`.
  - **Card header alignment** (the "разное выравнивание"): one `align-items:center` row — 15px
    radio, then the device name at `flex:1` **14.5px/600**, then the timestamp right-aligned in
    mono **10.5px**, `--sy-accent` when selected and `--sy-text-3` when not.
  - **Field rows**: mono 10px uppercase label (`letter-spacing:0.1em`), then the value in
    **mono 12.5px**. Differing values get a chip — `border-radius:7px`,
    `1px solid var(--sy-accent-border)`, `background var(--sy-surface)`, padding `8px 11px`.
    Identical values are **unboxed and dimmed to `--sy-text-3`**. The current `.conflict__cell`
    uses `--sy-radius-xs`, `--sy-border-strong` and 13.5px — align all three.
  - Keep the inline reveal button for secret fields (the mockup's conflict data has no secrets,
    but Закон №1 requires the affordance); restyle it to sit in the label row without breaking the
    mockup's rhythm.
  - Preserve current behaviour: nothing preselected, `chosenLabel`, `differingLine`.

---

## Phase 4 — Forms, sections, devices

- [ ] **4.1 Styled section picker in the record form** *(user item 12)*
  `RecordForm.vue:375-382` already uses `SySelect`, so it inherits the styled dropdown from phase
  1.3. Pass each section's colour as the option `dot`, and use the `#footer` slot for
  **«Управление секциями…»** (`Прототип:1768`) routing to `/sections`. Keep the existing
  `vaultOptions` / `vaultHint` logic (`RecordForm.vue:70-93`).

- [ ] **4.2 Drop the "dumb button" next to the notes field** *(user item 13)*
  Today the form renders a `<textarea>` plus a sibling `SyButton` «Показать текущие» nudged down
  22px, and the same shape for password («Показать текущий») and TOTP. Replace with the **card's
  own idiom**: when editing a record that has stored notes which have not been loaded, the field
  renders as the *hidden* state — skeleton bars plus a «Показать» chip inside the box — and
  clicking it loads the current value into the textarea. One affordance, inside the field, no
  sibling button, no magic offset. Apply the same treatment to the password and TOTP "load
  current" buttons.
  Keep the mockup's box geometry (`Прототип:1884`): `min-height:72px`, `--sy-radius-sm`,
  `1px solid var(--sy-border)`, `background var(--sy-surface)`, padding `11px 12px`, 13.5px, label
  **«Заметки · хранятся как секрет»**. The mockup uses an `<input>`; **keep the textarea** — that
  is the deliberate improvement the user asked to preserve.
  Watch `RecordForm.spec.ts:73`, which matches the button by the exact string
  `'Показать текущий'` (the notes variant is the distinct string `'Показать текущие'`).

- [ ] **4.3 TOTP field in the edit form** *(user item 14)*
  `Прототип:1888-1906`, the right half of a `1fr 1fr` grid with gap 20px. Label
  **«Код TOTP · необязательно»**. Two states, not a plain `SyInput`:
  - **empty**: `min-height:72px`, `--sy-radius-sm`, **`1px dashed var(--sy-border-strong)`**, no
    fill, padding `11px 12px`, two buttons with gap 12px — «Сканировать QR» (32px,
    `border-radius:7px`, `--sy-border-strong` / `--sy-surface`, 12.5px, hover `--sy-surface-2`) and
    a borderless accent «Ввести ключ вручную» (hover `background var(--sy-surface)`).
  - **attached**: `1px solid var(--sy-accent-border)`, `background var(--sy-accent-quiet)`, a
    30×30 `border-radius:8px` "QR" badge in mono 9px accent, title 13px/500 **«Ключ добавлен»**,
    subtitle mono 11px `--sy-text-2` **«секрет скрыт · код появится в карточке»**, and a 30px
    «Убрать» (hover `border-color: var(--sy-danger); color: var(--sy-danger)`).

  «Сканировать QR» has no core command behind it in MVP 1 — wire it to the manual-key path and say
  so in `TASKS.md` rather than shipping a dead button.

- [ ] **4.4 Right-pane header bars use the pane's own background** *(user item 15)*
  `SectionsView.vue:423` and `SettingsView.vue:418` set `background: var(--sy-bg-0)` — the sidebar
  chassis colour. In the mockup (`Прототип:1914`, `2027`, `2085`) the header has **no background
  of its own**: it inherits the pane's `var(--bg1)` and is separated only by
  `border-bottom: 1px solid var(--bd)`. Remove the `background` declaration from all three
  right-pane headers (Секции, Устройства, Настройки) so they read as one surface. Padding
  `22px 28px 18px`, title 22px/600 `letter-spacing:-0.01em`, subtitle 13px `--sy-text-2`.

- [ ] **4.5 New-section form is one row with a row of swatches** *(user item 16)*
  `SectionEditor.vue`. The mockup (`Прототип:1979-2008`) is **one horizontal row**,
  `align-items:flex-end`, gap 12: name field (`flex:1`) | colour block | «Создать» | «Отмена».
  - **Swatches must be a row.** `.section-editor__colors` is `flex-direction: column`; the
    swatches only line up today because each is `display:inline-block` with a `margin-right`. Make
    the fieldset a column of (legend, swatch row), with the swatch row
    `display:flex; gap:8px; height:38px; align-items:center`, and drop the per-swatch margin.
  - **Selection must not shift layout.** Today `--on` goes `1px` → `2px` border, nudging
    neighbours. The mockup has the same visual (`2px solid var(--tx)` vs `1px solid var(--bd)`) —
    reproduce it with `box-shadow: inset 0 0 0 2px` or an `outline` so geometry stays put.
    Swatch 24×24, `border-radius:7px`.
  - **Button sizes.** Both buttons **38px** (matching the form field height so the row bottoms
    align), `--sy-radius-sm`; primary padding `0 16px`, 13px/600, accent fill; secondary padding
    `0 14px`, 13px, on `--sy-surface` / `--sy-border-strong`. Today they are `size="sm"` (30px).
  - **Card chrome**: `1px solid var(--sy-accent)`, `--sy-radius`, `background var(--sy-bg-0)`,
    padding `14px 16px`, `box-shadow: 0 0 0 3px var(--sy-accent-quiet)`.
  - **Collapsed CTA**: full-width 44px, `--sy-radius`, `1px dashed var(--sy-border-strong)`,
    transparent, 13.5px «Новая секция», hover `color var(--sy-text)` +
    `border-color var(--sy-accent)`.
  - **The missing rule text.** The mockup carries the governing rules as two explainer cards below
    the list (`Прототип:2011-2020`): **«Удаление секции не удаляет записи»** / «Записи вернутся во
    «Все записи» и останутся на всех ваших устройствах…» and **«Одна запись — одна секция»** /
    «Секция выбирается в карточке записи выпадающим списком. Новые секции появляются в нём сразу,
    без перезапуска.» Verify the three existing note cards carry this text verbatim. The only
    in-form hint in the mockup is the placeholder «Например, Учёба» — keep the length hint too.

  `SectionsView.spec.ts` triggers `submit` on `.section-editor`, so it must stay a `<form>` with
  that class.

- [ ] **4.6 Devices — presence language, row geometry, hover, revoke dialog** *(user item 18)*
  - **Last-sync info.** The mockup never prints a raw timestamp; it is presence language plus a
    dot (`Прототип:2554-2558`): **«рядом · 12:04»**, **«рядом · только что»**,
    **«не в сети 3 недели»**, **«это устройство»** — with a 6px dot in `--sy-accent` when live and
    `--sy-text-3` when stale. Rewrite `deviceSubtitle` (`deviceFormat.ts`) to this vocabulary. Put
    the **fingerprint** under the name as mono 11px `--sy-text-3` in `XXXX · XXXX · XXXX` form.
  - **Row geometry** (`Прототип:2045-2065`): `--sy-radius`, `1px solid var(--sy-border)`,
    `background var(--sy-surface)`, padding `14px 16px`, gap 14; 38×38 tile (`--sy-radius`,
    `--sy-border-strong`, `background var(--sy-bg-1)`); name 14.5px/500; the "это устройство" chip
    mono 10px accent with `--sy-accent-border`, `border-radius:5px`, padding `2px 7px`.
  - **Button hover — the thing to check.** «Отозвать» is **neutral at rest**: 32px,
    `border-radius:7px`, `1px solid var(--sy-border-strong)`, `background var(--sy-bg-1)`,
    `color var(--sy-text-2)` — turning red **only on hover**
    (`border-color: var(--sy-danger); color: var(--sy-danger)`). The same rule applies to
    «Удалить» in the sections list. Verify neither is red at rest.
  - **Revoke dialog.** `RevokeDeviceModal.vue` is already the codebase's cleanest confirm
    (`size="confirm" tone="danger"`). Check it against `Прототип:2502-2516`: 480px, gap 14, title
    20px/600, body 13.5px `--sy-text-2` `line-height:1.55`, the note **«Подтвердите повторным
    нажатием «Отозвать»»** in the `#note` slot, buttons «Отмена» / danger «Отозвать».
  - Keep the two explainer cards «Отпечаток сверяется глазами» / «Что делает отзыв».

- [ ] **4.7 Pairing modal — check buttons, typography, geometry** *(user item 19; keep the better implementation)*
  Against `Прототип:2298-2376`: the modal is **`size="wizard"` (560px)**, not `wide` — check
  `PairingModal.vue:144`. Header `padding:16px 20px` with `border-bottom`, title 16px/600 plus a
  mono 10.5px step label, and a 30×30 close button (`border-radius:7px`, `--sy-border`,
  `background var(--sy-bg-1)`, hover `--sy-surface-2`). Body `padding:24px`, centred column, gap
  16. Footer buttons **38px**, secondary `flex:1`, primary `padding:0 16px`. The mode switch is
  currently two `SyButton`s toggling `variant` — an ad-hoc segmented control; replace it with the
  mockup's footer button pair. Fingerprint: mono 13px, `letter-spacing:0.12em`. Keep the QR plate
  always-light (`--sy-qr-bg` / `--sy-qr-ink`) — that is already correct.

---

## Phase 5 — Settings

- [ ] **5.1 New "Оформление" tab: theme + accent palette** *(user item 24)*
  `SettingsView.vue:59-63`. **No mockup exists** for this surface, so build it in the established
  settings-row idiom (`Прототип:2142-2159`): a row card with a 14.5px/500 title and a 12.5px
  `--sy-text-2` rationale on the left, and segmented 32px mono option buttons on the right — i.e.
  reuse `.settings__option` / `.settings__choice` exactly as the Безопасность pane does.
  - Add `{ id: 'appearance', name: 'Оформление' }` to `TABS` and to `readTab`.
  - **Theme row**: Тёмная / Светлая / Системная, backed by the existing `useTheme()` (`setTheme`
    already exists; `cycleTheme` becomes unused — remove it and its test).
  - **Accent row**: Мята / Циан / Янтарь / Индиго, backed by a new `accent` mode in `useTheme.ts`
    writing `data-accent` on `<html>` and persisting to `localStorage['syncra.accent']`.
    `syncra.theme` is currently the only key the frontend touches — this is the second, and it
    holds no secret. Palettes come from phase 1.1. Render the options as accent-coloured swatches.
  - **Remove `SyThemeToggle` from the titlebar** (`AppWindow.vue:73-76`) and delete the component.
    Its doc comment explains it was a deliberate consolidation, so record the new home in
    `TASKS.md`. Update `App.spec.ts` / `WindowControls.spec.ts` if they assert it.
  - `SettingsView.spec.ts` indexes `.settings__option` rows 0/1/2 → autolock/clipboard/reveal.
    Adding a tab does not disturb that, but adding rows to the *security* pane would — do not.
  - While here: the tab strip has `role="tablist"` / `role="tab"` but no `aria-controls` or
    `role="tabpanel"` wiring, so the panes are not announced as tab panels. Fix it.

- [ ] **5.2 "Данные" tab — typography, buttons, geometry** *(user item 21)*
  `SettingsView.vue` data pane against `Прототип:2101-2138`. Three cards, each
  `border-radius:10px; padding:18px; display:flex; align-items:flex-start; gap:16px`, with a
  `flex:1` text column (title **15px/600**, body **13px** `--sy-text-2` `line-height:1.55`) and a
  `flex:none` **36px** button:

  | Card | Container | Button |
  |---|---|---|
  | Импорт из другого менеджера | `--sy-border` / `--sy-surface` | accent solid, «Выбрать файл» |
  | Зашифрованный бэкап | `--sy-border` / `--sy-surface` | ghost `--sy-border-strong` / `--sy-bg-1`, 13px/500, «Создать бэкап» |
  | Экспорт в CSV | **`1px solid var(--sy-danger)` / `var(--sy-danger-quiet)`** | `1px solid var(--sy-danger)`, transparent, `color var(--sy-danger)`, 13px/600, hover solid + `--sy-danger-fg`, «Экспортировать CSV» |

  Receipts are mono 11px lines *inside* the card: `сохранено · syncra-backup-….enc` in
  `--sy-accent`, and `сохранено · syncra-export.csv · удалите файл после переноса` in
  `--sy-danger`. Closing note card: `--sy-border` / `var(--sy-bg-0)`, padding 16, 12.5px
  `--sy-text-2`, verbatim «Ни импорт, ни экспорт не обращаются к сети. Единственный канал, по
  которому данные покидают это устройство, — прямая синхронизация с вашими же устройствами в одной
  локальной сети.» `SettingsView.spec.ts` asserts `.settings__data-row` index 2 carries
  `--danger` — keep the order.

- [ ] **5.3 Backup export dialog — no reveal, no duplicate title, compact shape** *(user item 22)*
  `BackupModal.vue` + `BackupCard.vue`. No dialog for this exists in the mockups (the design fires
  the export immediately) and the password field is a contract requirement, so this stays a
  documented deviation — but three fixes:
  - **Remove the reveal control.** Pass `:revealable="false"` on the master-password `SyInput`
    (`BackupCard.vue:63-71`). `SyInput` defaults `revealable: true`, which is where the «Показать»
    chip comes from. Do the same in `CsvExportCard.vue`, `MasterPasswordModal.vue` and
    `UnlockView.vue` — mockup password inputs have no adornment anywhere
    (`Прототип:168, 1164, 1168, 1319, 2237, 2243`).
  - **Kill the duplicated title.** The modal renders «Зашифрованный бэкап» as its `title` *and*
    again as `.backup__title`, plus a "Спокойный вариант" pill that only makes sense on the
    standalone spec page. Drop the in-card header when the card is inside a modal.
  - **Stop the sausage.** Fold the four spec rows (Формат / Чем закрыт / Что внутри / Размер) into
    a two-column definition grid instead of four stacked bordered rows, and move the action button
    into `SyModal`'s `#actions`. Target a compact `size="form"` (520px) card.

- [ ] **5.4 CSV export dialog — less frightening, more compact** *(user item 23)*
  `CsvExportModal.vue` + `CsvExportCard.vue`. The mockup's dialog (`Прототип:2485-2500`) is
  **500px, flat, ~250px tall, four blocks**:
  1. title «Экспорт в CSV», 20px/600
  2. an **amber** warn strip — `1px solid var(--sy-warn)`, `background var(--sy-warn-quiet)`,
     `border-radius:8px`, padding `12px 14px`, 7px warn dot, 13px `--sy-text` — carrying verbatim:
     **«Файл не будет зашифрован: пароли внутри читаются как обычный текст. Удалите его сразу
     после переноса.»**
  3. 13.5px `--sy-text-2`: «Для резервной копии лучше подойдёт зашифрованный бэкап — он
     открывается только вашим мастер-паролем.»
  4. right-aligned «Отмена» + danger-ghost «Сохранить CSV» (hover fills solid)

  Note the deliberate token split: **the card border is neutral `--sy-border-strong`, the alert is
  `--warn`, and only the confirm button is `--dang`.** That alone removes most of the "оправданно
  страшный, но слишком высокий" feeling. The current implementation stacks three gates — a
  checkbox acknowledgement, a typed `ЭКСПОРТ` confirmation *and* a master-password field — plus a
  hatched `repeating-linear-gradient` background. The heavier two-gate version exists in
  `Syncra Data and Settings.dc.html:275-320` as a **full spec page, not a dialog**. **Keep exactly
  one gate in the dialog**: the master password, which the contract requires. Drop the checkbox,
  the typed word and the hatching. Keep the post-export receipt with the «Удалить файл сейчас»
  button — that is a real safety affordance and matches the spec page. `dataModals.spec.ts`
  asserts `[data-test="csv-modal"]`; keep the hook and expect to rewrite the gate assertions.

- [ ] **5.5 Confirm the out-of-scope areas were left alone**
  User item 17 (sections management plate) and user item 20 (password/security settings) are
  explicitly better than the mockup and must not be touched. Tick this box only after verifying
  no diff landed in them.

---

## Extra discrepancies found during review — not in the user's list

Report these in the final summary. Boxes 1-4 are already covered by tasks above; tick them there
and here.

- [X] **E1. `--sy-radius-md` does not exist.** The token ladder is xs/sm/(unnamed)/lg/pill, so
  `.card__menu-pop` renders at radius 0. This is the *cause* of user item 10, not a separate skin
  issue — worth stating because the same class of bug (an undefined token silently rendering as
  nothing) can recur. Grep for others. *(Covered by 1.1.)*
- [ ] **E2. The copy verb is inconsistent** across the card: `SySecretField` hardcodes
  «Копировать» while metadata rows say «Скопировать адрес/логин». *(Covered by 2.5.)*
- [ ] **E3. The delete dialog does not use `size="confirm"`** despite `SyModal`'s own prop doc
  naming that size for exactly this case. *(Covered by 2.9.)*
- [ ] **E4. `margin-top: 22px` appears three times** as a label-height hack. *(Covered by 1.4.)*
- [ ] **E5. `SyModal` has no focus trap and does not restore focus on close** — a real
  accessibility gap in a security product. *(Suggested in 1.5.)*
- [ ] **E6. `SyListItem` exposes an `#actions` slot that `RecordList` ignores**, absolutely
  positioning `.record-list__copy` instead, with a hardcoded `border-radius: 7px`. Fold into phase
  1.2's icon-button work.
  > NOTE: the slot cannot be used as described, and `RecordList` is not at fault. The row is itself
  > a `<button>` wrapping `SyListItem` (`RecordList.vue:266-279`), so anything in the slot would be
  > a button nested inside a button — invalid markup, and a click on it would also open the record.
  > The comment at `RecordList.vue:281-284` says exactly this. What is left of E6 is real but
  > smaller: the hardcoded `border-radius: 7px` (now `--sy-radius-tag`'s neighbour
  > `--sy-radius-inner`, added in 1.1), and the fact that `SyListItem`'s `#actions` slot has no
  > callers anywhere in `src/` — it is dead API and should probably be deleted. Deliberately left
  > for phase 2, which restructures this file anyway (2.1, 2.2).
- [ ] **E7. `TrustedDevices.vue` has a stray blank line (170) and an out-of-order import** —
  lint-adjacent noise; tidy while in the file.

---

## Verification

Run after each phase, not only at the end:

```
npm run typecheck
npm run lint
npm run test
```

Node ≥ 22.18 is required for the jsdom tests (see `engines`). If this machine is on Node 20 the
DOM tests will not start; the known workaround is `npm i --no-save jsdom@25`.

### Test files that will need updating

They bind tightly to BEM class names and to button text:

- `RecordList.spec.ts` — ~20 class assertions; `.record-list__hotkey` disappears (task 2.2).
- `RecordCard.spec.ts` and `VaultShell.spec.ts` — both match `.card__foot .sy-button--danger`;
  keep the delete button in the card footer so these survive.
- `RecordForm.spec.ts` (50 tests) — matches `'Показать текущий'` / `'Показать текущие'` by exact
  text; task 4.2 removes those buttons, so rewrite against the new in-field affordance.
- `SettingsView.spec.ts` — depends on `.settings__option` row order and on `.settings__data-row`
  index 2 carrying `--danger`.
- `SectionsView.spec.ts` — triggers `submit` on `.section-editor`.
- `ConflictDialog.spec.ts`, `dataModals.spec.ts`, `components.spec.ts`, `useTheme.spec.ts`.
- **New tests**: sort order in `useRecordList` / `useRecordsStore`; `get_totp_code` in the mock;
  the accent switch in `useTheme`; and a Закон №1 test proving the TOTP **code** never reaches a
  store.

### Manual pass against the mock core

`npm run dev`, in both themes and at least two accents:

unlock → list (search fills the column, sort actually reorders) → card (34px buttons aligned, no
Версия block, TOTP ticking in 3+3, notes showing skeletons, footer counting devices) → "···"
(round corners) → delete dialog (order, no overflow, amber caveat inside a red card) → conflict
dialog (banded) → edit form (styled section dropdown, no sibling «Показать» buttons, dashed TOTP
block) → sections (header matches the pane, swatches in a row, 38px buttons) → devices (presence
language, revoke red only on hover) → settings (four tabs, Оформление switches theme and accent,
compact CSV dialog, no «Показать» chip on master-password fields).

Confirm in the built bundle: zero external URLs, no crypto in the frontend, `localStorage` holding
only `syncra.theme` and `syncra.accent`, and no secret field in any store snapshot.

### Contract change to record in `TASKS.md` and confirm with the backend agent

`get_totp_code { record_id } → { code, seconds_left, period_s }` — the code is generated by the
core. The frontend must never receive the TOTP secret in order to display a code.
