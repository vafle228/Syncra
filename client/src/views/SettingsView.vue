<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import BackupModal from '@/components/data/BackupModal.vue'
import CsvExportModal from '@/components/data/CsvExportModal.vue'
import ImportModal from '@/components/data/ImportModal.vue'
import GeneratorProfileForm from '@/components/generator/GeneratorProfileForm.vue'
import MasterPasswordModal from '@/components/security/MasterPasswordModal.vue'
import { SyButton } from '@/components/ui'
import { securityPolicy } from '@/composables/securityPolicy'
import {
  ACCENTS,
  useTheme,
  type AccentPreference,
  type ThemePreference,
} from '@/composables/useTheme'
import { useClipboard } from '@/composables/useClipboard'
import {
  useDebounced,
  useGeneratorProfileDraft,
  usePasswordGenerator,
} from '@/composables/usePasswordGenerator'
import {
  AUTOLOCK_OPTIONS_MS,
  CLIPBOARD_CLEAR_OPTIONS_MS,
  SECRET_REVEAL_OPTIONS_MS,
  type GeneratorProfile,
  type SecuritySettingsPatch,
  type VaultId,
} from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSecurityStore } from '@/stores/useSecurityStore'
import { useToastStore } from '@/stores/useToastStore'
import { useVaultUiStore } from '@/stores/useVaultUiStore'

/**
 * Настройки (F6, F13, §3.11 макета) — три вкладки одной правой панели.
 *
 * Порядок вкладок не случаен: «Безопасность» первая, потому что это единственная
 * вкладка, где выбор меняет то, как продукт защищает человека. Генератор — про
 * удобство, данные — про редкие операции.
 *
 * ЗАКОН №1: профиль и таймауты — это правила, а не секреты, и они спокойно живут
 * в сторах. Пароль-пример рядом с генератором — уже пароль: он приходит разово
 * из ядра, лежит в области видимости экрана и исчезает при уходе с него. Сами
 * файлы импорта и экспорта собирает и разбирает ядро.
 */

const route = useRoute()
const router = useRouter()
const records = useRecordsStore()
const security = useSecurityStore()
const ui = useVaultUiStore()
const toast = useToastStore()

// ---------------------------------------------------------------------------
// Вкладки
//
// Адрес — единственная правда о том, какая вкладка открыта: так на неё можно
// дать ссылку, а `Back` не шагает по вкладкам (`replace`, а не `push`).
// ---------------------------------------------------------------------------

const TABS = [
  { id: 'security', name: 'Безопасность' },
  { id: 'generator', name: 'Генератор' },
  { id: 'data', name: 'Данные' },
  { id: 'appearance', name: 'Оформление' },
] as const

type TabId = (typeof TABS)[number]['id']

/** Старые ссылки `/data?tab=import|export` вели сюда же — принимаем их. */
function readTab(raw: unknown): TabId {
  if (raw === 'generator') return 'generator'
  if (raw === 'appearance') return 'appearance'
  if (raw === 'data' || raw === 'import' || raw === 'export') return 'data'
  return 'security'
}

const tab = ref<TabId>(readTab(route.query.tab))

/**
 * Связка вкладки и панели для диктора. Без `aria-controls`/`role="tabpanel"`
 * полоса вкладок озвучивается как набор кнопок, а панель — как «что-то ниже»:
 * связи между ними нет, и переход по вкладке ничего не сообщает.
 */
const tabId = (id: TabId): string => `settings-tab-${id}`
const panelId = (id: TabId): string => `settings-panel-${id}`

watch(
  () => route.query.tab,
  (raw) => {
    tab.value = readTab(raw)
  },
)

function pickTab(next: TabId): void {
  tab.value = next
  void router.replace({ name: 'settings', query: next === 'security' ? {} : { tab: next } })
}

// ---------------------------------------------------------------------------
// Оформление
//
// Макета у этой вкладки нет: до сих пор тема переключалась кнопкой в полосе
// заголовка, а акцент не переключался вовсе. Собрана она в том же идиоме, что
// и «Безопасность» (`Прототип:2142-2159`): строка-карточка с объяснением слева
// и сегментом вариантов справа.
// ---------------------------------------------------------------------------

const { preference: themePreference, setTheme, accent, setAccent } = useTheme()

const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
  { value: 'dark', label: 'Тёмная' },
  { value: 'light', label: 'Светлая' },
  { value: 'system', label: 'Системная' },
]

const ACCENT_LABELS: Record<AccentPreference, string> = {
  mint: 'Мята',
  cyan: 'Циан',
  amber: 'Янтарь',
  indigo: 'Индиго',
}

const accentOptions = ACCENTS.map((value) => ({ value, label: ACCENT_LABELS[value] }))

// ---------------------------------------------------------------------------
// Безопасность
// ---------------------------------------------------------------------------

/** Подпись варианта: «1 мин», «5 мин», «30 с». Секунды и минуты — не одно и то же. */
function durationLabel(ms: number): string {
  if (ms < 60_000) return `${Math.round(ms / 1000)} с`
  return `${Math.round(ms / 60_000)} мин`
}

const securityRows = computed(() => [
  {
    key: 'autolock_ms' as const,
    name: 'Автоблокировка',
    // Считает бездействие ЯДРО. Фронт эти числа только показывает — второй
    // таймер означал бы вторую правду о том, заперто ли хранилище.
    description:
      'Через сколько бездействия хранилище запрётся само. Считает ядро, а не окно: свернули или заперли крышку — отсчёт продолжается.',
    options: AUTOLOCK_OPTIONS_MS,
    value: security.settings.autolock_ms,
  },
  {
    key: 'clipboard_clear_ms' as const,
    name: 'Очистка буфера обмена',
    description:
      'Скопированный пароль стирается из буфера сам. Пока он там лежит, его может прочитать любая программа на этом компьютере.',
    options: CLIPBOARD_CLEAR_OPTIONS_MS,
    value: security.settings.clipboard_clear_ms,
  },
  {
    key: 'secret_reveal_ms' as const,
    name: 'Показ секрета на экране',
    description:
      'Через сколько открытый пароль скроется обратно. Отсчёт начинается в момент показа и не продлевается.',
    options: SECRET_REVEAL_OPTIONS_MS,
    value: security.settings.secret_reveal_ms,
  },
])

const savingKey = ref<string | null>(null)
const securityError = ref<string | null>(null)

async function setSecurity(key: keyof SecuritySettingsPatch, value: number): Promise<void> {
  if (savingKey.value !== null) return

  savingKey.value = key
  securityError.value = null
  try {
    await security.save({ [key]: value } as SecuritySettingsPatch)
    toast.push('Настройка сохранена', 'success')
  } catch (cause) {
    securityError.value = isCoreError(cause)
      ? cause.message
      : 'Не удалось сохранить настройку безопасности.'
  } finally {
    savingKey.value = null
  }
}

const changingMaster = ref(false)

// ---------------------------------------------------------------------------
// Генератор (F6)
// ---------------------------------------------------------------------------

const profile = useGeneratorProfileDraft()
const preview = usePasswordGenerator()
const clipboard = useClipboard()

/** Пример — ровно один: это не выбор варианта, а иллюстрация правил. */
function reroll(): void {
  void preview.generate(1, profile.draft.value ?? undefined)
}

const rerollSoon = useDebounced(reroll)

const example = computed(() => preview.variants.value[0] ?? '')

onMounted(async () => {
  void security.ensure()
  await profile.ensure()
  reroll()
})

// Правило поменяли — пример пересобирается по НОВЫМ правилам, ещё до
// сохранения: иначе «пример по текущим правилам» показывал бы прошлые.
watch(
  () => profile.draft.value,
  (next, previous) => {
    if (previous !== null && next !== null) rerollSoon()
  },
  { deep: true },
)

function setProfile(next: GeneratorProfile): void {
  profile.set(next)
}

async function save(): Promise<void> {
  if (await profile.save()) toast.push('Профиль генератора сохранён', 'success')
}

/** Копируем как секрет: буфер очистится сам по действующей политике. */
async function copyExample(): Promise<void> {
  if (example.value === '') return

  const seconds = Math.ceil(securityPolicy().value.clipboard_clear_ms / 1000)
  const done = await clipboard.copy('example', example.value, {
    clearAfterMs: securityPolicy().value.clipboard_clear_ms,
  })
  if (done) toast.push(`Пароль в буфере · очистится через ${seconds} с`, 'success')
  else toast.push('Буфер обмена недоступен', 'danger')
}

// ---------------------------------------------------------------------------
// Данные (F12)
// ---------------------------------------------------------------------------

const importing = ref(false)
const backing = ref(false)
const exporting = ref(false)

/**
 * След последнего экспорта — в карточке, а не в тосте (`Прототип:2118`,
 * `2129`). Тост уедет через три секунды, а файл на диске останется; про
 * незашифрованный CSV об этом надо помнить дольше.
 *
 * Хранится только имя файла: это не секрет, но и не содержимое — путь целиком
 * показывает сам диалог, пока он открыт.
 */
const backupFile = ref<string | null>(null)
const csvFile = ref<string | null>(null)

/** Имя файла из пути: разделители у платформ разные. */
function fileName(path: string): string {
  return path.split(/[\/]/).pop() ?? path
}

/**
 * Импорт закончился — показываем, что приехало. Фильтр списка ставим на свежую
 * секцию: 300 чужих записей вперемешку со своими нельзя ни проверить, ни
 * разобрать. Баннер над списком договаривает главное: они пока только здесь.
 */
async function onImported(vaultId: VaultId, count: number): Promise<void> {
  importing.value = false
  records.setVaultFilter(vaultId)
  await records.load()
  ui.showImportBanner(count)
  await router.push({ name: 'home' })
}
</script>

<template>
  <main class="settings">
    <!-- «Назад» нет: сайдбар с паролями никуда не уходил (F13). -->
    <header class="settings__header">
      <div class="settings__brand">
        <h1 class="settings__title">Настройки</h1>
        <p class="settings__lead">
          Каждый пункт объясняет цену выбора, а не просто включает галочку.
        </p>
      </div>

      <div class="settings__tabs" role="tablist" aria-label="Разделы настроек">
        <button
          v-for="item in TABS"
          :key="item.id"
          type="button"
          :id="tabId(item.id)"
          role="tab"
          class="settings__tab"
          :class="{ 'settings__tab--on': tab === item.id }"
          :aria-selected="tab === item.id"
          :aria-controls="panelId(item.id)"
          :tabindex="tab === item.id ? undefined : -1"
          :data-test="`tab-${item.id}`"
          @click="pickTab(item.id)"
        >
          {{ item.name }}
        </button>
      </div>
    </header>

    <div class="settings__body">
      <!-- Безопасность -->
      <section
        v-if="tab === 'security'"
        :id="panelId('security')"
        class="settings__pane"
        role="tabpanel"
        :aria-labelledby="tabId('security')"
        data-test="pane-security"
      >
        <p v-if="security.error" class="settings__error" role="alert">{{ security.error }}</p>
        <p v-if="securityError" class="settings__error" role="alert">{{ securityError }}</p>

        <div v-for="row in securityRows" :key="row.key" class="settings__option">
          <div class="settings__option-text">
            <span class="settings__option-name">{{ row.name }}</span>
            <span class="settings__option-desc">{{ row.description }}</span>
          </div>

          <div class="settings__choices" role="group" :aria-label="row.name">
            <button
              v-for="option in row.options"
              :key="option"
              type="button"
              class="settings__choice"
              :class="{ 'settings__choice--on': row.value === option }"
              :aria-pressed="row.value === option"
              :disabled="savingKey !== null"
              @click="setSecurity(row.key, option)"
            >
              {{ durationLabel(option) }}
            </button>
          </div>
        </div>

        <div class="settings__master">
          <p class="settings__master-text">
            Смена мастер-пароля перешифровывает хранилище на этом устройстве и требует подтверждения
            на остальных — они спросят новый пароль при следующей встрече.
          </p>
          <SyButton size="sm" data-test="master-password-open" @click="changingMaster = true">
            Сменить мастер-пароль
          </SyButton>
        </div>
      </section>

      <!-- Оформление -->
      <section
        v-else-if="tab === 'appearance'"
        :id="panelId('appearance')"
        class="settings__pane"
        role="tabpanel"
        :aria-labelledby="tabId('appearance')"
        data-test="pane-appearance"
      >
        <div class="settings__option">
          <div class="settings__option-text">
            <span class="settings__option-name">Тема</span>
            <span class="settings__option-desc">
              Системная следует за настройкой ОС и переключается вместе с ней. Тёмная и светлая
              фиксируют выбор независимо от системы.
            </span>
          </div>

          <div class="settings__choices" role="group" aria-label="Тема">
            <button
              v-for="option in THEME_OPTIONS"
              :key="option.value"
              type="button"
              class="settings__choice"
              :class="{ 'settings__choice--on': themePreference === option.value }"
              :aria-pressed="themePreference === option.value"
              @click="setTheme(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <div class="settings__option">
          <div class="settings__option-text">
            <span class="settings__option-name">Акцент</span>
            <span class="settings__option-desc">
              Цвет, которым помечено выбранное и открытое. На то, что и как хранится, он не влияет —
              это единственная настройка здесь, у которой нет цены.
            </span>
          </div>

          <div class="settings__choices" role="group" aria-label="Акцент">
            <button
              v-for="option in accentOptions"
              :key="option.value"
              type="button"
              class="settings__choice settings__accent"
              :class="{ 'settings__choice--on': accent === option.value }"
              :aria-pressed="accent === option.value"
              @click="setAccent(option.value)"
            >
              <span class="settings__accent-dot" :data-accent="option.value" aria-hidden="true" />
              {{ option.label }}
            </button>
          </div>
        </div>
      </section>

      <!-- Генератор -->
      <section
        v-else-if="tab === 'generator'"
        :id="panelId('generator')"
        class="settings__pane"
        role="tabpanel"
        :aria-labelledby="tabId('generator')"
        data-test="pane-generator"
      >
        <div class="settings__intro">
          <h2 class="settings__pane-title">Профиль генератора</h2>
          <p class="settings__pane-text">
            Это правило, по которому Syncra предлагает пароль в каждой новой записи. Настроили один
            раз — дальше в форме будет просто готовый пароль и кнопка «другой».
          </p>
        </div>

        <p v-if="profile.loadError.value" class="settings__error" role="alert">
          {{ profile.loadError.value }}
        </p>

        <div class="settings__preview">
          <div class="settings__preview-head">
            <span class="settings__preview-label">Пример по текущим правилам</span>
            <span class="settings__preview-entropy">≈ {{ preview.entropyBits.value }} бит</span>
          </div>

          <p v-if="preview.error.value" class="settings__error" role="alert">
            {{ preview.error.value }}
          </p>
          <p v-else class="settings__example">{{ example || '…' }}</p>

          <div class="settings__preview-actions">
            <SyButton size="sm" :loading="preview.busy.value" @click="reroll">
              Другой пример
            </SyButton>
            <SyButton
              variant="primary"
              size="sm"
              :disabled="example === '' || !clipboard.available.value"
              @click="copyExample"
            >
              {{ clipboard.copiedKey.value === 'example' ? 'Скопирован' : 'Скопировать' }}
            </SyButton>
            <span class="settings__preview-note">случайность берётся у ОС</span>
          </div>
        </div>

        <GeneratorProfileForm
          v-if="profile.draft.value"
          :model-value="profile.draft.value"
          :dirty="profile.dirty.value"
          :saving="profile.saving.value"
          :error="profile.saveError.value"
          @update:model-value="setProfile"
          @save="save"
        />
        <p v-else-if="profile.loading.value" class="settings__pane-text">Загружаем правила…</p>
      </section>

      <!-- Данные -->
      <section
        v-else
        :id="panelId('data')"
        class="settings__pane"
        role="tabpanel"
        :aria-labelledby="tabId('data')"
        data-test="pane-data"
      >
        <div class="settings__data-row">
          <div class="settings__data-text">
            <span class="settings__data-title">Импорт из другого менеджера</span>
            <span class="settings__data-body">
              Файл читается на этом устройстве и никуда не отправляется. После импорта его стоит
              удалить — внутри пароли открытым текстом.
            </span>
          </div>
          <SyButton variant="primary" data-test="open-import" @click="importing = true">
            Выбрать файл
          </SyButton>
        </div>

        <div class="settings__data-row">
          <div class="settings__data-text">
            <span class="settings__data-title">Зашифрованный бэкап</span>
            <span class="settings__data-body">
              Один файл, который открывается только вашим мастер-паролем. Подходит для флешки в
              ящике стола.
            </span>
            <span v-if="backupFile" class="settings__data-receipt">
              сохранено · {{ backupFile }}
            </span>
          </div>
          <SyButton data-test="open-backup" @click="backing = true">Создать бэкап</SyButton>
        </div>

        <div class="settings__data-row settings__data-row--danger">
          <div class="settings__data-text">
            <span class="settings__data-title">Экспорт в CSV</span>
            <span class="settings__data-body">
              Файл не шифруется: пароли внутри читаются как обычный текст. Нужен только для переезда
              в другой менеджер.
            </span>
            <span v-if="csvFile" class="settings__data-receipt settings__data-receipt--danger">
              сохранено · {{ csvFile }} · удалите файл после переноса
            </span>
          </div>
          <SyButton variant="danger" data-test="open-csv" @click="exporting = true">
            Экспортировать CSV
          </SyButton>
        </div>

        <p class="settings__data-note">
          Ни импорт, ни экспорт не обращаются к сети. Единственный канал, по которому данные
          покидают это устройство, — прямая синхронизация с вашими же устройствами в одной локальной
          сети.
        </p>
      </section>
    </div>

    <MasterPasswordModal :open="changingMaster" @close="changingMaster = false" />
    <ImportModal :open="importing" @close="importing = false" @imported="onImported" />
    <BackupModal
      :open="backing"
      @close="backing = false"
      @done="backupFile = fileName($event.path)"
    />
    <CsvExportModal
      :open="exporting"
      @close="exporting = false"
      @done="csvFile = fileName($event.path)"
      @forgotten="csvFile = null"
    />
  </main>
</template>

<style scoped>
/*
 * Экран — правая панель окна (F13), а не отдельная страница: он занимает
 * остаток ширины и высоту окна, а прокручивается его тело, оставляя сайдбар и
 * полосу заголовка на месте.
 */
.settings {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  background: var(--sy-bg-1);
}

/*
 * У шапки НЕТ своей заливки: в макете (`Прототип:1914`, `2027`, `2085`) она
 * часть панели и отделена только линией. `--sy-bg-0` — цвет шасси окна, и на
 * правой панели он читался как чужая полоса, приклеенная сверху.
 */
.settings__header {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
  padding: var(--sy-space-7) var(--sy-space-8) 0;
  border-bottom: 1px solid var(--sy-border);
}

.settings__brand {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.settings__title {
  font-size: 22px;
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.settings__lead {
  font-size: var(--sy-text-note);
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/*
 * Свотч акцента красит себя ТЕМ ЖЕ атрибутом `data-accent`, что и документ:
 * образец обязан показывать ровно ту палитру, которую включит нажатие, и
 * второй список цветов рядом с токенами разъехался бы с ними при первой правке.
 */
.settings__accent-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--sy-accent);
}

.settings__accent {
  display: inline-flex;
  align-items: center;
  gap: var(--sy-space-3);
}

/* Вкладки: подчёркивание, а не кнопки — это навигация внутри экрана. */
.settings__tabs {
  display: flex;
  gap: var(--sy-space-2);
}

.settings__tab {
  height: 38px;
  padding: 0 var(--sy-space-5);
  border: none;
  border-bottom: 2px solid transparent;
  background: transparent;
  color: var(--sy-text-2);
  font-family: inherit;
  font-size: 13.5px;
  cursor: pointer;
  transition: color var(--sy-transition);
}

.settings__tab:hover {
  color: var(--sy-text);
}

.settings__tab:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.settings__tab--on {
  border-bottom-color: var(--sy-accent);
  color: var(--sy-text);
  font-weight: var(--sy-weight-semibold);
}

.settings__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--sy-space-8);
}

.settings__pane {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  max-width: 820px;
}

.settings__intro {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.settings__pane-title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.settings__pane-text {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/* Безопасность */

.settings__option {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--sy-space-5) var(--sy-space-7);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.settings__option-text {
  flex: 1;
  min-width: 260px;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.settings__option-name {
  font-size: 14.5px;
  font-weight: var(--sy-weight-medium);
}

.settings__option-desc {
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.settings__choices {
  flex: none;
  display: flex;
  gap: var(--sy-space-2);
}

.settings__choice {
  height: 32px;
  padding: 0 var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-xs);
  background: var(--sy-bg-1);
  color: var(--sy-text-3);
  font-family: var(--sy-font-mono);
  font-size: 12px;
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.settings__choice:hover:not(:disabled) {
  border-color: var(--sy-border-strong);
  color: var(--sy-text-2);
}

.settings__choice:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.settings__choice:disabled {
  cursor: progress;
}

.settings__choice--on {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.settings__master {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--sy-space-6);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
}

.settings__master-text {
  flex: 1;
  min-width: 260px;
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/* Генератор */

.settings__preview {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius);
  background: var(--sy-accent-quiet);
}

.settings__preview-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-5);
}

.settings__preview-label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.settings__preview-entropy {
  font-family: var(--sy-font-mono);
  font-size: 11.5px;
  color: var(--sy-text-2);
}

.settings__example {
  font-family: var(--sy-font-mono);
  font-size: 19px;
  letter-spacing: 0.02em;
  overflow-wrap: anywhere;
}

.settings__preview-actions {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  flex-wrap: wrap;
}

.settings__preview-note {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

/* Данные */

.settings__data-row {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  gap: var(--sy-space-6);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

/* Опасное не спрятано, но и не выглядит как безобидное. */
.settings__data-row--danger {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
}

.settings__data-text {
  flex: 1;
  min-width: 260px;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.settings__data-title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.settings__data-body {
  font-size: var(--sy-text-note);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/*
 * След экспорта живёт В КАРТОЧКЕ, а не в тосте: тост уедет, а файл на диске
 * останется — и про CSV об этом надо помнить дольше трёх секунд.
 */
.settings__data-receipt {
  padding-top: 2px;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  color: var(--sy-accent);
  word-break: break-all;
}

.settings__data-receipt--danger {
  color: var(--sy-danger);
}

.settings__data-note {
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.settings__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>
