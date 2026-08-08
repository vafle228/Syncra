<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'

import GeneratorProfileForm from '@/components/generator/GeneratorProfileForm.vue'
import { SyButton } from '@/components/ui'
import { securityPolicy } from '@/composables/securityPolicy'
import { useClipboard } from '@/composables/useClipboard'
import {
  useDebounced,
  useGeneratorProfileDraft,
  usePasswordGenerator,
} from '@/composables/usePasswordGenerator'
import type { GeneratorProfile } from '@/core/contract'
import { useToastStore } from '@/stores/useToastStore'

/**
 * Настройки (F6, §3.11 макета).
 *
 * Две секции: профиль генератора (F6) и данные (F12) — вход в импорт, бэкап и
 * CSV-экспорт. Сами потоки живут на `/data`: здесь у каждого только честный
 * ценник, и опасное помечено словами, а не одним цветом.
 *
 * ЗАКОН №1: профиль — это правила, а не секрет, и он спокойно живёт в сторе.
 * Пароль-пример рядом с ним — уже пароль: он приходит разово из ядра, лежит в
 * области видимости этого экрана и исчезает при уходе с него.
 */

const profile = useGeneratorProfileDraft()
const preview = usePasswordGenerator()
const clipboard = useClipboard()
const toast = useToastStore()

/**
 * Данные (F12, §3.11 макета). Сами потоки живут на `/data` — здесь только
 * вход в них и честный ценник у каждого: «безопасно» и «открытый текст»
 * стоят рядом, но выглядят по-разному.
 */
const dataRows = [
  {
    title: 'Импорт из другого менеджера',
    tag: 'безопасно',
    body: 'Файл читается локально и удаляется сразу после разбора.',
    action: 'Открыть импорт',
    tab: 'import',
    danger: false,
  },
  {
    title: 'Зашифрованный бэкап',
    tag: 'безопасно',
    body: 'Копия хранилища под мастер-паролем — можно хранить где угодно.',
    action: 'Сделать бэкап',
    tab: 'export',
    danger: false,
  },
  {
    title: 'Экспорт в CSV',
    tag: 'открытый текст',
    body: 'Пароли без шифрования. Только для переезда, с удалением файла сразу после.',
    action: 'Открыть экспорт',
    tab: 'export',
    danger: true,
  },
]

/** Пример — ровно один: это не выбор варианта, а иллюстрация правил. */
function reroll(): void {
  void preview.generate(1, profile.draft.value ?? undefined)
}

const rerollSoon = useDebounced(reroll)

const example = computed(() => preview.variants.value[0] ?? '')

onMounted(async () => {
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

/** Копируем как секрет: буфер очистится сам через 20 с. */
async function copyExample(): Promise<void> {
  if (example.value === '') return

  const done = await clipboard.copy('example', example.value, {
    clearAfterMs: securityPolicy().value.clipboard_clear_ms,
  })
  if (done) toast.push('Пароль в буфере · очистится через 20 с', 'success')
  else toast.push('Буфер обмена недоступен', 'danger')
}
</script>

<template>
  <main class="settings">
    <!-- «Назад» нет: сайдбар с паролями никуда не уходил (F13). -->
    <header class="settings__header">
      <div class="settings__brand">
        <h1 class="settings__title">Настройки</h1>
      </div>
    </header>

    <div class="settings__body">
      <section class="settings__pane">
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

      <section class="settings__pane">
        <div class="settings__intro">
          <h2 class="settings__pane-title">Данные</h2>
          <p class="settings__pane-text">
            Три действия с разной ценой ошибки, поэтому и выглядят они по-разному. Опасное не
            спрятано, но и не соседствует с безобидным.
          </p>
        </div>

        <ul class="settings__data">
          <li
            v-for="row in dataRows"
            :key="row.title"
            class="settings__data-row"
            :class="{ 'settings__data-row--danger': row.danger }"
          >
            <div class="settings__data-text">
              <span class="settings__data-head">
                <span class="settings__data-title">{{ row.title }}</span>
                <span class="settings__data-tag">{{ row.tag }}</span>
              </span>
              <span class="settings__data-body">{{ row.body }}</span>
            </div>

            <RouterLink class="settings__data-link" :to="{ name: 'data', query: { tab: row.tab } }">
              {{ row.action }}
            </RouterLink>
          </li>
        </ul>
      </section>
    </div>
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

.settings__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.settings__brand {
  display: flex;
  align-items: baseline;
  gap: var(--sy-space-6);
}

.settings__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.settings__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--sy-space-9) var(--sy-space-8);
}

.settings__pane {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-8);
  max-width: 720px;
  margin: 0 auto;
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
  letter-spacing: -0.015em;
}

.settings__pane-text {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.settings__preview {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-7);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius);
  background: var(--sy-accent-quiet);
}

.settings__preview-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
}

.settings__preview-label {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-accent);
}

.settings__preview-entropy {
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-2);
}

.settings__example {
  font-family: var(--sy-font-mono);
  font-size: 22px;
  line-height: 1.35;
  word-break: break-all;
}

.settings__preview-actions {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
}

.settings__preview-note {
  margin-left: auto;
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.settings__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

.settings__data {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  margin: 0;
  padding: 0;
  list-style: none;
}

.settings__data-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-6) var(--sy-space-7);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
}

.settings__data-row--danger {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
}

.settings__data-head {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
}

.settings__data-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.settings__data-tag {
  padding: 2px var(--sy-space-4);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-pill);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--sy-accent);
}

.settings__data-row--danger .settings__data-tag {
  border-color: var(--sy-danger);
  color: var(--sy-danger);
}

.settings__data-body {
  display: block;
  padding-top: var(--sy-space-1);
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.settings__data-link {
  flex: none;
  display: inline-flex;
  align-items: center;
  height: var(--sy-control-height-sm);
  padding: 0 var(--sy-space-6);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  font-size: var(--sy-text-body);
  color: var(--sy-text);
  text-decoration: none;
}

.settings__data-row--danger .settings__data-link {
  border-color: var(--sy-danger);
  background: transparent;
  color: var(--sy-danger);
}
</style>
