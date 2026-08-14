<script setup lang="ts">
import { computed, ref } from 'vue'

import { SyButton, SyToggle } from '@/components/ui'
import { pluralize, RECORD_FORMS } from '@/composables/plural'
import { useVaultImport } from '@/composables/useDataTransfer'
import type { ImportSource } from '@/core/contract'
import { IMPORT_SOURCE_INFO, IMPORT_STATUS_LABEL, importSourceInfo } from './importSources'

/**
 * Мастер импорта из чужого менеджера (F12, §3.10 макета).
 *
 * ЗАКОН №1 работает в обе стороны. Файл выбирает, читает и разбирает ЯДРО;
 * сюда приходят только сайт, логин и статус строки — в макете это сказано
 * прямым текстом: «прочитан локально · пароли пока не показываем». Чужие
 * пароли ждут согласия в ядре и забываются по отмене.
 */

/**
 * Сколько записей приехало — вместе с секцией, в которую они легли. Число нужно
 * баннеру над списком: он называет его вслух, и брать его повторным пересчётом
 * списка значило бы иметь две правды об одном импорте.
 */
const emit = defineEmits<{ imported: [vaultId: string, count: number] }>()

const source = ref<ImportSource>('chrome')
const wizard = useVaultImport()

const info = computed(() => importSourceInfo(source.value))

const hiddenRows = computed(() => {
  const preview = wizard.preview.value
  if (preview === null) return 0
  return Math.max(0, preview.total_rows - preview.rows.length)
})

/** Сколько записей заведётся при текущих настройках. */
const willImport = computed(() => {
  const preview = wizard.preview.value
  if (preview === null) return 0
  return preview.new_count + (wizard.options.value.skip_duplicates ? 0 : preview.duplicate_count)
})

const stats = computed(() => {
  const preview = wizard.preview.value
  if (preview === null) return []
  return [
    { tag: 'Найдено', n: preview.total_rows, sub: 'строк в файле', tone: 'plain' },
    { tag: 'Новых', n: preview.new_count, sub: 'попадут в хранилище', tone: 'accent' },
    { tag: 'Дубликаты', n: preview.duplicate_count, sub: 'уже есть такие пары', tone: 'plain' },
    { tag: 'Без пароля', n: preview.no_password_count, sub: 'пустое поле в файле', tone: 'warn' },
  ]
})

function pickSource(next: ImportSource): void {
  source.value = next
  void wizard.cancel()
}

async function chooseFile(): Promise<void> {
  await wizard.pick(source.value)
}

async function runImport(): Promise<void> {
  await wizard.commit()
}

function again(): void {
  wizard.reset()
}

function showImported(): void {
  const done = wizard.result.value
  if (done) emit('imported', done.vault.vault_id, done.imported)
}
</script>

<template>
  <section class="import">
    <!-- Шаг 3: перенос состоялся. -->
    <div v-if="wizard.result.value" class="import__done">
      <h3 class="import__done-title">
        {{ pluralize(wizard.result.value.imported, RECORD_FORMS) }} на месте
      </h3>
      <p class="import__done-text">
        Лежат в секции «{{ wizard.result.value.vault.name }}» — переложите по своим, когда будет
        время.
      </p>
      <p v-if="wizard.result.value.skipped > 0" class="import__done-text">
        {{ pluralize(wizard.result.value.skipped, RECORD_FORMS) }} пропущено: такие пары «адрес +
        логин» в Syncra уже были.
      </p>
      <p v-if="wizard.result.value.source_file_deleted" class="import__done-note">
        Исходный файл удалён с диска.
      </p>

      <div v-if="wizard.result.value.reused_passwords > 0" class="import__reused">
        <span class="import__reused-title">
          {{ wizard.result.value.reused_passwords }} паролей повторяются на разных сайтах
        </span>
        <span class="import__reused-body">
          Это наследство прошлого менеджера, не срочно. Менять их по одному в неделю — комфортный
          темп.
        </span>
      </div>

      <div class="import__actions">
        <SyButton variant="primary" @click="showImported">Показать импортированные</SyButton>
        <SyButton @click="again">Импортировать ещё</SyButton>
      </div>
    </div>

    <!-- Шаг 2: файл разобран, показываем, что попадёт внутрь. -->
    <div v-else-if="wizard.preview.value" class="import__preview">
      <header class="import__preview-head">
        <div>
          <h3 class="import__preview-title">Файл разобран. Посмотрите, что попадёт внутрь</h3>
          <p class="import__preview-sub">
            {{ wizard.preview.value.file_name }} · прочитан локально · пароли пока не показываем
          </p>
        </div>
        <SyButton size="sm" :disabled="wizard.busy.value" @click="chooseFile">Другой файл</SyButton>
      </header>

      <ul class="import__stats">
        <li
          v-for="stat in stats"
          :key="stat.tag"
          class="import__stat"
          :class="`import__stat--${stat.tone}`"
        >
          <span class="import__stat-tag">{{ stat.tag }}</span>
          <span class="import__stat-n">{{ stat.n }}</span>
          <span class="import__stat-sub">{{ stat.sub }}</span>
        </li>
      </ul>

      <table class="import__table">
        <thead>
          <tr>
            <th>Сайт</th>
            <th>Логин</th>
            <th>Что сделаем</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, index) in wizard.preview.value.rows" :key="`${row.site}-${index}`">
            <td>{{ row.site }}</td>
            <td class="import__login">{{ row.login }}</td>
            <td>
              <span class="import__status" :class="`import__status--${row.status}`">
                {{ IMPORT_STATUS_LABEL[row.status] }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-if="hiddenRows > 0" class="import__more">
        и ещё {{ pluralize(hiddenRows, RECORD_FORMS) }}
      </p>

      <div class="import__options">
        <SyToggle
          :model-value="wizard.options.value.skip_duplicates"
          label="Пропустить дубликаты"
          description="Совпал сайт и логин — оставим то, что уже есть в Syncra."
          @update:model-value="wizard.options.value.skip_duplicates = $event"
        />
        <SyToggle
          :model-value="wizard.options.value.flag_reused"
          label="Пометить повторяющиеся пароли"
          description="Не блокирует импорт: просто соберём список, чтобы разобраться позже."
          @update:model-value="wizard.options.value.flag_reused = $event"
        />
      </div>

      <p class="import__target">
        Всё ляжет в новую секцию «{{ wizard.preview.value.target_vault_name }}» — разберёте потом,
        не смешивая с текущим.
      </p>

      <p v-if="wizard.error.value" class="import__error" role="alert">{{ wizard.error.value }}</p>

      <div class="import__actions">
        <SyButton
          variant="primary"
          :loading="wizard.busy.value"
          :disabled="willImport === 0"
          @click="runImport"
        >
          Импортировать {{ pluralize(willImport, RECORD_FORMS) }}
        </SyButton>
        <SyButton :disabled="wizard.busy.value" @click="wizard.cancel">Отмена</SyButton>
      </div>
    </div>

    <!-- Шаг 1: откуда переносим. -->
    <div v-else class="import__pick">
      <div class="import__sources">
        <span class="import__caption">Откуда переносим</span>
        <button
          v-for="item in IMPORT_SOURCE_INFO"
          :key="item.key"
          type="button"
          class="import__source"
          :class="{ 'import__source--on': item.key === source }"
          :aria-pressed="item.key === source"
          @click="pickSource(item.key)"
        >
          <span class="import__source-tag">{{ item.tag }}</span>
          <span class="import__source-text">
            <span class="import__source-name">{{ item.name }}</span>
            <span class="import__source-sub">{{ item.sub }}</span>
          </span>
        </button>
      </div>

      <div class="import__how">
        <h3 class="import__how-title">{{ info.title }}</h3>
        <p class="import__how-lead">{{ info.lead }}</p>

        <ol class="import__steps">
          <li v-for="(step, index) in info.steps" :key="step" class="import__step">
            <span class="import__step-n">{{ index + 1 }}</span>
            <span>{{ step }}</span>
          </li>
        </ol>

        <button
          type="button"
          class="import__drop"
          :disabled="wizard.busy.value"
          @click="chooseFile"
        >
          <span class="import__drop-title">Выберите {{ info.file }}</span>
          <span class="import__drop-sub"
            >окно выбора откроет Syncra — файл не покинет компьютер</span
          >
        </button>

        <p v-if="wizard.error.value" class="import__error" role="alert">{{ wizard.error.value }}</p>

        <p class="import__note">
          Файл разбирается прямо на этом компьютере и удаляется сразу после импорта. Ни строки не
          уходит в сеть — переносить пароли через чужой сервер было бы странно для менеджера без
          сервера.
        </p>
      </div>
    </div>
  </section>
</template>

<style scoped>
.import {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
}

.import__pick {
  display: grid;
  grid-template-columns: 232px minmax(0, 1fr);
  gap: var(--sy-space-8);
  align-items: start;
}

.import__caption {
  display: block;
  padding-bottom: var(--sy-space-4);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.import__sources {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.import__source {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  padding: var(--sy-space-4) var(--sy-space-5);
  border: 1px solid transparent;
  border-radius: var(--sy-radius-sm);
  background: transparent;
  text-align: left;
  cursor: pointer;
  color: inherit;
  font: inherit;
}

.import__source:hover {
  background: var(--sy-surface);
}

.import__source--on {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
}

.import__source-tag {
  display: grid;
  place-items: center;
  flex: none;
  width: 30px;
  height: 30px;
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-2);
}

.import__source-name {
  display: block;
  font-size: var(--sy-text-body);
}

.import__source-sub {
  display: block;
  font-size: 11.5px;
  color: var(--sy-text-3);
}

.import__how {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
}

.import__how-title,
.import__preview-title,
.import__done-title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.015em;
}

.import__how-lead,
.import__done-text {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.import__steps {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  margin: 0;
  padding: 0;
  list-style: none;
}

.import__step {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  font-size: 13px;
  color: var(--sy-text-2);
}

.import__step-n {
  display: grid;
  place-items: center;
  flex: none;
  width: 22px;
  height: 22px;
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-inner);
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-3);
}

.import__drop {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
  padding: var(--sy-space-9) var(--sy-space-7);
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
  cursor: pointer;
  color: inherit;
  font: inherit;
  text-align: center;
}

.import__drop:hover:not(:disabled) {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
}

.import__drop-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
}

.import__drop-sub,
.import__note,
.import__done-note,
.import__target {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.import__preview,
.import__done {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
}

.import__preview-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sy-space-6);
}

.import__preview-sub {
  padding-top: var(--sy-space-2);
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-3);
}

.import__stats {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--sy-space-4);
  margin: 0;
  padding: 0;
  list-style: none;
}

.import__stat {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-bg-0);
}

.import__stat--accent {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
}

.import__stat--warn {
  border-color: var(--sy-warn);
  background: var(--sy-warn-quiet);
}

.import__stat-tag {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.import__stat--accent .import__stat-tag {
  color: var(--sy-accent);
}

.import__stat--warn .import__stat-tag {
  color: var(--sy-warn);
}

.import__stat-n {
  font-family: var(--sy-font-mono);
  font-size: 21px;
}

.import__stat-sub {
  font-size: 11.5px;
  color: var(--sy-text-3);
}

.import__table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.import__table th {
  padding: 0 var(--sy-space-5) var(--sy-space-3);
  border-bottom: 1px solid var(--sy-border);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  font-weight: var(--sy-weight-regular);
  letter-spacing: 0.1em;
  text-transform: uppercase;
  text-align: left;
  color: var(--sy-text-3);
}

.import__table td {
  padding: var(--sy-space-4) var(--sy-space-5);
  border-bottom: 1px solid var(--sy-border);
}

.import__login {
  color: var(--sy-text-2);
}

.import__status {
  display: inline-flex;
  padding: 2px var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-pill);
  font-size: 11.5px;
  color: var(--sy-text-3);
}

.import__status--new {
  border-color: var(--sy-accent-border);
  color: var(--sy-accent);
}

.import__status--no_password {
  border-color: var(--sy-warn);
  color: var(--sy-warn);
}

.import__more {
  font-size: 12.5px;
  color: var(--sy-text-3);
}

.import__options {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
}

.import__reused {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  padding: var(--sy-space-6) var(--sy-space-7);
  border: 1px solid var(--sy-warn);
  border-radius: var(--sy-radius);
  background: var(--sy-warn-quiet);
}

.import__reused-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.import__reused-body {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
}

.import__actions {
  display: flex;
  gap: var(--sy-space-4);
}

.import__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

@media (max-width: 900px) {
  .import__pick {
    grid-template-columns: minmax(0, 1fr);
  }

  .import__stats {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
