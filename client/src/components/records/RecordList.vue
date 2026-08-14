<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

import { SyButton, SyEmptyState, SyListItem, SySelect } from '@/components/ui'
import type { SySelectOption } from '@/components/ui'
import { ACCOUNT_FORMS, pluralize, RECORD_FORMS, SERVICE_FORMS } from '@/composables/plural'
import type { RecordOrder } from '@/composables/useRecordList'
import { securityPolicy } from '@/composables/securityPolicy'
import { useRecordSecrets } from '@/composables/useRecordSecrets'
import type { RecordId } from '@/core/contract'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useSyncStore } from '@/stores/useSyncStore'
import { useToastStore } from '@/stores/useToastStore'
import { useVaultUiStore } from '@/stores/useVaultUiStore'

/**
 * Средняя панель окна: поиск, счётчик и список записей (F4, §3.1).
 *
 * ЗАКОН №1: в списке нет и не будет секретов — только `RecordMeta`. Поиск идёт
 * по метаданным; содержимое паролей и заметок не индексируется, и это
 * проговаривается пользователю в пустом состоянии поиска.
 *
 * Единственное место, где панель вообще касается секрета, — кнопка копирования
 * на выбранной строке: значение приходит разово, уходит в буфер и на экране не
 * появляется (`useRecordSecrets.copy`). В разметку оно не попадает никогда.
 *
 * Панель монтируется только на роутах с `meta.listPane` — вместе с ней исчезает
 * и хоткей поиска, что правильно: искать по списку, которого нет, нечего.
 */

const list = useRecordsStore()
const sections = useSectionsStore()
const sync = useSyncStore()
const conflicts = useConflictsStore()
const ui = useVaultUiStore()
const toast = useToastStore()

/**
 * Отметка в строке списка (F10, F11). Конфликт важнее ожидания отправки: он
 * ждёт человека, а «ждёт синхронизации» разрешится само.
 */
function rowStatus(id: RecordId): 'none' | 'pending' | 'conflict' {
  if (conflicts.byRecord(id) !== null) return 'conflict'
  return sync.isPending(id) ? 'pending' : 'none'
}

const searchInput = ref<HTMLInputElement | null>(null)

/** Хранилище открыто, ядро ответило, но записей в нём нет вообще. */
const isVaultEmpty = computed(() => list.loaded && list.totalAll === 0)
/** Записей в хранилище хватает, но выбранная секция пуста. */
const isSectionEmpty = computed(
  () => list.loaded && list.totalAll > 0 && list.vaultFilter !== null && list.total === 0,
)
/** Записи есть, но под запрос не подошла ни одна. */
const isSearchEmpty = computed(() => list.loaded && list.total > 0 && list.visible === 0)

/** Выбранная секция — или `null`, когда показаны все записи. */
const currentSection = computed(() => sections.byId(list.vaultFilter))

/**
 * Строка счётчика над списком.
 *
 * «пароли скрыты» — обещание прототипа, и оно стоит здесь, а не в подсказке:
 * это главное, что отличает список Syncra от таблицы паролей, и человек должен
 * читать это каждый раз, а не один раз в онбординге.
 */
const countLine = computed(() => {
  if (!list.loaded) return ''
  if (list.isSearching) return `найдено ${list.visible} из ${list.total} · пароли скрыты`

  const records = pluralize(list.total, RECORD_FORMS)
  const services = pluralize(list.serviceCount, SERVICE_FORMS)
  return `${records} · ${services} · пароли скрыты`
})

function groupCount(count: number): string {
  return pluralize(count, ACCOUNT_FORMS)
}

/**
 * Порядок списка (`Прототип:1427`). Три варианта, и третий — не про удобство, а
 * про гигиену: «старые пароли» показывают, что давно не меняли.
 */
const ORDER_OPTIONS: (SySelectOption & { value: RecordOrder })[] = [
  { value: 'recent', label: 'Недавние' },
  { value: 'alpha', label: 'По алфавиту' },
  { value: 'stale', label: 'Старые пароли' },
]

/** Список отдаёт строку; в стор уходит только знакомый нам ключ порядка. */
function changeOrder(value: string): void {
  const next = ORDER_OPTIONS.find((option) => option.value === value)
  if (next) list.setOrder(next.value)
}

const importText = computed(() =>
  ui.importBanner === null ? '' : `Перенесено ${pluralize(ui.importBanner.count, RECORD_FORMS)}`,
)

// ---------------------------------------------------------------------------
// Копирование пароля прямо из строки
//
// Composable заводится на ВЫБРАННУЮ запись, потому что кнопка есть только у неё:
// смена выбора сама гасит всё, что успело открыться (`watch(recordId)` внутри).
// ---------------------------------------------------------------------------

const secrets = useRecordSecrets(computed(() => list.selectedId))

async function copyPassword(): Promise<void> {
  const done = await secrets.copy('password')
  if (done) {
    const seconds = Math.ceil(securityPolicy().value.clipboard_clear_ms / 1000)
    toast.push(`Пароль в буфере · очистится через ${seconds} с`, 'success')
  } else if (secrets.error.value === null) {
    toast.push('Буфер обмена недоступен', 'danger')
  }
}

// ---------------------------------------------------------------------------
// Поиск
// ---------------------------------------------------------------------------

function focusSearch(): void {
  searchInput.value?.focus()
  searchInput.value?.select()
}

/** Ctrl/Cmd + K — фокус в поиск (подсказка есть в макете). */
function onKeydown(event: KeyboardEvent): void {
  if (event.key.toLowerCase() === 'k' && (event.ctrlKey || event.metaKey)) {
    event.preventDefault()
    focusSearch()
  }
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div class="record-list" data-test="list-pane">
    <div class="record-list__toolbar">
      <div class="record-list__toolbar-row">
        <div class="record-list__search">
          <span class="record-list__search-icon" aria-hidden="true" />
          <input
            ref="searchInput"
            class="record-list__search-input"
            type="search"
            autocomplete="off"
            spellcheck="false"
            placeholder="Поиск по сервису, логину и адресу"
            aria-label="Поиск по сервису, логину и адресу"
            :value="list.query"
            @input="list.setQuery(($event.target as HTMLInputElement).value)"
            @keydown.escape="list.clearQuery()"
          />
        </div>

        <button
          type="button"
          class="record-list__new"
          title="Новая запись"
          aria-label="Новая запись"
          data-test="toolbar-new"
          @click="ui.startCreate()"
        >
          <span aria-hidden="true" />
        </button>
      </div>

      <!--
        Справа от счётчика — порядок списка. Подсказки про Ctrl+K тут больше нет:
        строки «Ctrl» нет ни в одном макете, а сам хоткей остался — он ничего не
        занимает на экране.
      -->
      <div class="record-list__count">
        <span>{{ countLine }}</span>
        <SySelect
          class="record-list__order"
          variant="inline"
          aria-label="Порядок списка"
          :model-value="list.order"
          :options="ORDER_OPTIONS"
          data-test="list-order"
          @update:model-value="changeOrder"
        />
      </div>

      <!--
        Про локальную секцию говорим там, где её записи открывают, а не только в
        настройках: «эти пароли есть только здесь» — то, что стоит знать до того,
        как на них понадеешься.
      -->
      <p v-if="currentSection && !currentSection.sync" class="record-list__local" role="status">
        Секция «{{ currentSection.name }}» локальная — эти записи не уезжают с этого устройства.
      </p>
    </div>

    <div class="record-list__body">
      <!--
        Импорт закончился. Баннер объясняет не «сколько приехало», а то, чего
        человек не видит: записи ещё нигде, кроме этого компьютера.
      -->
      <div v-if="ui.importBanner" class="record-list__import" role="status">
        <p class="record-list__import-title">{{ importText }}</p>
        <p class="record-list__import-text">
          Они помечены как «только на этом устройстве», пока не разойдутся по вашим устройствам.
        </p>
        <button type="button" class="record-list__import-close" @click="ui.hideImportBanner()">
          Понятно
        </button>
      </div>

      <p v-if="list.error" class="record-list__note record-list__note--error" role="alert">
        {{ list.error }}
      </p>

      <!--
        Скелет, а не спиннер: ключ уже в памяти, читается локальный файл —
        спиннер во весь экран врал бы про долгую загрузку. Показывается до
        первого успешного ответа; повторная загрузка список уже не мигает.
      -->
      <div v-else-if="!list.loaded" class="record-list__skeleton" aria-hidden="true">
        <div v-for="row in 4" :key="row" class="record-list__skeleton-row">
          <span class="record-list__skeleton-icon" />
          <span class="record-list__skeleton-lines">
            <span class="record-list__skeleton-line" />
            <span class="record-list__skeleton-line record-list__skeleton-line--short" />
          </span>
        </div>
      </div>

      <SyEmptyState
        v-else-if="isVaultEmpty"
        title="Пока ни одного пароля"
        description="Хранилище создано и лежит на этом компьютере. Осталось наполнить его — вручную или переносом из другого менеджера."
      >
        <template #actions>
          <SyButton variant="primary" size="sm" @click="ui.startCreate()">Новая запись</SyButton>
        </template>
      </SyEmptyState>

      <SyEmptyState
        v-else-if="isSectionEmpty"
        title="В этой секции пока пусто"
        description="Записи появятся здесь, когда вы создадите их с этой секцией или перенесёте существующие — секция выбирается в форме записи."
      >
        <template #actions>
          <SyButton size="sm" @click="ui.startCreate()">Новая запись</SyButton>
          <SyButton size="sm" @click="list.setVaultFilter(null)">Показать все</SyButton>
        </template>
      </SyEmptyState>

      <SyEmptyState
        v-else-if="isSearchEmpty"
        title="Ничего не нашлось"
        :description="`По запросу «${list.query.trim()}» нет ни сервиса, ни логина, ни адреса. Поиск смотрит только внутрь этого устройства и не заглядывает в секреты.`"
      >
        <template #actions>
          <SyButton size="sm" @click="list.clearQuery()">Сбросить поиск</SyButton>
        </template>
      </SyEmptyState>

      <div v-else class="record-list__list">
        <!--
          Группа §4.4 — чисто визуальная: заголовок появляется только там, где у
          сервиса правда несколько аккаунтов. Одиночная запись остаётся обычной
          строкой, без лишней рамки вокруг неё.
        -->
        <section
          v-for="group in list.groups"
          :key="group.key"
          class="record-list__group"
          :class="{ 'record-list__group--multi': group.records.length > 1 }"
        >
          <header v-if="group.records.length > 1" class="record-list__group-head">
            <span class="record-list__group-name">{{ group.title }}</span>
            <span class="record-list__group-count">{{ groupCount(group.records.length) }}</span>
          </header>

          <ul class="record-list__group-list">
            <li
              v-for="record in group.records"
              :key="record.record_id"
              class="record-list__item"
              data-test="row"
            >
              <button
                type="button"
                class="record-list__row"
                :aria-current="list.selectedId === record.record_id ? 'true' : undefined"
                @click="ui.openRecord(record.record_id)"
              >
                <SyListItem
                  :title="record.service_name"
                  :subtitle="record.login"
                  :badge="record.account_label"
                  :seed="record.urls[0] ?? record.service_name"
                  :selected="list.selectedId === record.record_id"
                  :status="rowStatus(record.record_id)"
                />
              </button>

              <!--
                Кнопка живёт РЯДОМ со строкой, а не внутри неё: вложенная кнопка
                — невалидная разметка, и клик по ней открывал бы заодно запись.
              -->
              <button
                v-if="list.selectedId === record.record_id"
                type="button"
                class="record-list__copy"
                title="Скопировать пароль"
                aria-label="Скопировать пароль"
                :disabled="secrets.busy.value !== null"
                @click="copyPassword()"
              >
                <span aria-hidden="true" />
              </button>
            </li>
          </ul>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.record-list {
  flex: none;
  width: var(--sy-list-width);
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: 1px solid var(--sy-border);
  background: var(--sy-bg-1);
}

/* Панель поиска */

.record-list__toolbar {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  padding: var(--sy-space-5);
  border-bottom: 1px solid var(--sy-border);
}

.record-list__toolbar-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
}

.record-list__new {
  flex: none;
  position: relative;
  width: 34px;
  height: 34px;
  border: 1px solid transparent;
  border-radius: var(--sy-radius-sm);
  background: var(--sy-accent);
  cursor: pointer;
  transition: filter var(--sy-transition);
}

.record-list__new:hover {
  filter: brightness(1.08);
}

.record-list__new span,
.record-list__new span::before {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 13px;
  height: 1.5px;
  background: var(--sy-accent-fg);
  transform: translate(-50%, -50%);
  content: '';
}

.record-list__new span::before {
  transform: translate(-50%, -50%) rotate(90deg);
}

.record-list__search {
  /* Поиск занимает всю строку, кроме 34×34 кнопки «+» (`Прототип:1417`). */
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  height: 34px;
  padding: 0 var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  transition:
    border-color var(--sy-transition),
    box-shadow var(--sy-transition);
}

.record-list__search:focus-within {
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.record-list__search-icon {
  flex: none;
  width: 11px;
  height: 11px;
  border: 1.5px solid var(--sy-text-3);
  border-radius: 50%;
}

.record-list__search-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--sy-text);
  font-family: inherit;
  font-size: var(--sy-text-body);
}

.record-list__search-input::placeholder {
  color: var(--sy-text-3);
}

.record-list__search-input::-webkit-search-cancel-button {
  display: none;
}

.record-list__count {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

/* Счётчик набран моноширинным; подпись порядка — нет, и шрифт ей надо вернуть. */
.record-list__order {
  flex: none;
  font-family: var(--sy-font-sans);
}

.record-list__local {
  padding: var(--sy-space-3) var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-xs);
  background: var(--sy-surface);
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/* Список */

.record-list__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.record-list__note {
  padding: var(--sy-space-8) var(--sy-space-6);
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
}

.record-list__note--error {
  color: var(--sy-danger);
}

.record-list__list {
  display: flex;
  flex-direction: column;
}

.record-list__group-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.record-list__group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-5) var(--sy-space-2);
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.record-list__group-count {
  flex: none;
  color: var(--sy-accent);
}

.record-list__item {
  position: relative;
}

/* Строка списка — кнопка: открывается и мышью, и с клавиатуры. */
.record-list__row {
  display: block;
  width: 100%;
  padding: 0;
  border: none;
  background: none;
  text-align: left;
  font: inherit;
  color: inherit;
  cursor: pointer;
}

.record-list__row:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

/*
 * Кнопка копирования выбранной строки. Абсолютная, потому что лежать внутри
 * строки-кнопки она не может, а занимать место в потоке рядом с ней — значит
 * дёргать вёрстку при каждом переходе по списку.
 */
.record-list__copy {
  position: absolute;
  top: 50%;
  right: var(--sy-space-4);
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-inner);
  background: transparent;
  cursor: pointer;
  transform: translateY(-50%);
  transition: background var(--sy-transition);
}

.record-list__copy:hover:not(:disabled) {
  background: var(--sy-accent-quiet);
}

.record-list__copy:disabled {
  cursor: progress;
  opacity: 0.6;
}

.record-list__copy:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.record-list__copy span {
  width: 11px;
  height: 11px;
  border: 1.5px solid var(--sy-accent);
  border-radius: 3px;
}

/* Освободить место под кнопку, чтобы она не легла на метку или на отметку. */
.record-list__item:has(> .record-list__copy) :deep(.sy-list-item) {
  padding-right: 48px;
}

/* Баннер импорта */

.record-list__import {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--sy-space-3);
  margin: var(--sy-space-4);
  padding: var(--sy-space-4) var(--sy-space-5);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-accent-quiet);
}

.record-list__import-title {
  font-size: 13px;
  font-weight: var(--sy-weight-semibold);
}

.record-list__import-text {
  font-size: var(--sy-text-small);
  line-height: 1.45;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.record-list__import-close {
  height: 28px;
  padding: 0 11px;
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-xs);
  background: transparent;
  color: var(--sy-accent);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
}

/* Скелет загрузки */

.record-list__skeleton {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  padding: var(--sy-space-5);
}

.record-list__skeleton-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  height: 48px;
  padding: 0 var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  /*
   * Своя анимация, а не общая `sy-pulse`: та пульсирует ещё и масштабом — это
   * верно для точки синхронизации и неверно для строки, которая должна стоять
   * на месте, пока грузится.
   */
  animation: record-list-shimmer 1.6s ease-in-out infinite;
}

.record-list__skeleton-row:nth-child(2) {
  animation-delay: 0.15s;
}

.record-list__skeleton-row:nth-child(3) {
  animation-delay: 0.3s;
}

.record-list__skeleton-row:nth-child(4) {
  animation-delay: 0.45s;
}

.record-list__skeleton-icon {
  flex: none;
  width: 28px;
  height: 28px;
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface-2);
}

.record-list__skeleton-lines {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.record-list__skeleton-line {
  width: 52%;
  height: 9px;
  border-radius: 3px;
  background: var(--sy-surface-2);
}

.record-list__skeleton-line--short {
  width: 38%;
  height: 8px;
}

@keyframes record-list-shimmer {
  0%,
  100% {
    opacity: 0.55;
  }
  50% {
    opacity: 1;
  }
}

/* Узкое окно: список перестаёт быть колонкой и встаёт над правой панелью. */
@media (max-width: 860px) {
  .record-list {
    width: auto;
    border-right: none;
    border-bottom: 1px solid var(--sy-border);
  }
}
</style>
