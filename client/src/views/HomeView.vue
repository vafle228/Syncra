<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import RecordCard from '@/components/records/RecordCard.vue'
import RecordForm from '@/components/records/RecordForm.vue'
import { SyButton, SyEmptyState, SyListItem, SyThemeToggle } from '@/components/ui'
import type { RecordId } from '@/core/contract'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useVaultStore } from '@/stores/useVaultStore'

/**
 * Главный экран: список записей слева, карточка записи справа (F4, F5, §3.1).
 *
 * ЗАКОН №1: в списке нет и не будет секретов — только `RecordMeta`. Поиск идёт
 * по метаданным; содержимое паролей и заметок не индексируется, и это
 * проговаривается пользователю в пустом состоянии поиска. Секреты появляются
 * только в правой панели и только по явному нажатию (см. `RecordCard`).
 *
 * Записи намеренно НЕ выбирается автоматически: открывать чью-то карточку без
 * спроса — не то, чего ждёшь от менеджера паролей на общем столе.
 *
 * Сайдбар секций — F7.
 */

const router = useRouter()
const vault = useVaultStore()
const list = useRecordsStore()

const searchInput = ref<HTMLInputElement | null>(null)

/** Что показывает правая панель. */
type Pane = 'view' | 'edit' | 'create'
const pane = ref<Pane>('view')

function openRecord(id: RecordId): void {
  list.select(id)
  pane.value = 'view'
}

function startCreate(): void {
  list.select(null)
  pane.value = 'create'
}

function cancelEditor(): void {
  pane.value = 'view'
}

// Пользователь ушёл искать другое — форма создания не должна висеть поверх.
watch(
  () => list.query,
  () => {
    if (pane.value === 'create') pane.value = 'view'
  },
)

/** Хранилище открыто, ядро ответило, но записей в нём нет вообще. */
const isVaultEmpty = computed(() => list.loaded && list.total === 0)
/** Записи есть, но под запрос не подошла ни одна. */
const isSearchEmpty = computed(() => list.loaded && list.total > 0 && list.visible === 0)

function plural(count: number, forms: [string, string, string]): string {
  const mod100 = count % 100
  const mod10 = count % 10
  if (mod100 >= 11 && mod100 <= 14) return forms[2]
  if (mod10 === 1) return forms[0]
  if (mod10 >= 2 && mod10 <= 4) return forms[1]
  return forms[2]
}

/** Строка счётчика над списком: в режиме поиска показывает «найдено N из M». */
const countLine = computed(() => {
  if (!list.loaded) return ''
  if (list.isSearching) return `найдено ${list.visible} из ${list.total}`

  const records = `${list.total} ${plural(list.total, ['запись', 'записи', 'записей'])}`
  const services = `${list.serviceCount} ${plural(list.serviceCount, ['сервис', 'сервиса', 'сервисов'])}`
  return `${records} · ${services}`
})

function groupCount(count: number): string {
  return `${count} ${plural(count, ['аккаунт', 'аккаунта', 'аккаунтов'])}`
}

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
  void list.load()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
})

async function lock(): Promise<void> {
  try {
    await vault.lock()
    await router.push({ name: 'unlock' })
  } catch {
    /* сообщение уже в vault.error */
  }
}
</script>

<template>
  <main class="home">
    <header class="home__header">
      <div class="home__brand">
        <span class="home__mark" aria-hidden="true"><span /></span>
        <h1 class="home__title">Syncra</h1>
      </div>

      <div class="home__header-actions">
        <SyThemeToggle />
        <RouterLink class="home__settings" :to="{ name: 'settings' }">Настройки</RouterLink>
        <SyButton size="sm" :disabled="vault.busy" @click="lock">Заблокировать</SyButton>
      </div>
    </header>

    <div class="home__panes">
      <div class="home__list-pane">
        <div class="home__toolbar">
          <div class="home__toolbar-row">
            <div class="home__search">
              <span class="home__search-icon" aria-hidden="true" />
              <input
                ref="searchInput"
                class="home__search-input"
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
              class="home__new"
              title="Новая запись"
              aria-label="Новая запись"
              @click="startCreate"
            >
              <span aria-hidden="true" />
            </button>
          </div>

          <div class="home__count">
            <span>{{ countLine }}</span>
            <span class="home__hotkey">Ctrl + K</span>
          </div>
        </div>

        <div class="home__body">
          <p v-if="list.error" class="home__note home__note--error" role="alert">
            {{ list.error }}
          </p>

          <!--
            Скелет, а не спиннер: ключ уже в памяти, читается локальный файл —
            спиннер во весь экран врал бы про долгую загрузку. Показывается до
            первого успешного ответа; повторная загрузка список уже не мигает.
          -->
          <div v-else-if="!list.loaded" class="home__skeleton" aria-hidden="true">
            <div v-for="row in 4" :key="row" class="home__skeleton-row">
              <span class="home__skeleton-icon" />
              <span class="home__skeleton-lines">
                <span class="home__skeleton-line" />
                <span class="home__skeleton-line home__skeleton-line--short" />
              </span>
            </div>
          </div>

          <SyEmptyState
            v-else-if="isVaultEmpty"
            title="Пока ни одного пароля"
            description="Хранилище создано и лежит на этом компьютере. Осталось наполнить его — вручную или переносом из другого менеджера."
          >
            <template #actions>
              <SyButton variant="primary" size="sm" @click="startCreate">Новая запись</SyButton>
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

          <div v-else class="home__list">
            <!--
              Группа §4.4 — чисто визуальная: заголовок появляется только там, где
              у сервиса правда несколько аккаунтов. Одиночная запись остаётся
              обычной строкой, без лишней рамки вокруг неё.
            -->
            <section
              v-for="group in list.groups"
              :key="group.key"
              class="home__group"
              :class="{ 'home__group--multi': group.records.length > 1 }"
            >
              <header v-if="group.records.length > 1" class="home__group-head">
                <span class="home__group-name">{{ group.title }}</span>
                <span class="home__group-count">{{ groupCount(group.records.length) }}</span>
              </header>

              <ul class="home__group-list">
                <li v-for="record in group.records" :key="record.record_id">
                  <button
                    type="button"
                    class="home__row"
                    :aria-current="list.selectedId === record.record_id ? 'true' : undefined"
                    @click="openRecord(record.record_id)"
                  >
                    <SyListItem
                      :title="record.service_name"
                      :subtitle="record.login"
                      :badge="record.account_label"
                      :seed="record.urls[0] ?? record.service_name"
                      :selected="list.selectedId === record.record_id"
                    />
                  </button>
                </li>
              </ul>
            </section>
          </div>
        </div>
      </div>

      <!--
        Правая панель. Форма и карточка получают `key` по записи: при переходе
        на другую запись компонент пересоздаётся, а вместе с ним обнуляется всё
        открытое — и черновик формы, и показанные секреты.
      -->
      <div class="home__detail">
        <RecordForm
          v-if="pane === 'create'"
          key="create"
          @saved="pane = 'view'"
          @cancel="cancelEditor"
        />

        <RecordForm
          v-else-if="pane === 'edit' && list.selected"
          :key="`edit:${list.selected.record_id}`"
          :record="list.selected"
          @saved="pane = 'view'"
          @cancel="cancelEditor"
        />

        <RecordCard
          v-else-if="list.selected"
          :key="list.selected.record_id"
          :record="list.selected"
          @edit="pane = 'edit'"
        />

        <SyEmptyState
          v-else
          class="home__detail-empty"
          title="Запись не выбрана"
          description="Выберите строку слева, чтобы посмотреть её. Пароль не появится на экране сам — только по нажатию."
        >
          <template #actions>
            <SyButton size="sm" @click="startCreate">Новая запись</SyButton>
          </template>
        </SyEmptyState>
      </div>
    </div>
  </main>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  background: var(--sy-bg-1);
}

.home__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.home__brand {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.home__mark {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border: 1.5px solid var(--sy-accent);
  border-radius: var(--sy-radius-sm);
}

.home__mark span {
  width: 9px;
  height: 9px;
  border-radius: 3px;
  background: var(--sy-accent);
}

.home__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.home__header-actions {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
}

.home__settings {
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  text-decoration: none;
}

.home__settings:hover {
  color: var(--sy-text);
  text-decoration: underline;
}

/* Две панели: список слева, карточка справа (§3.1 макета) */

.home__panes {
  flex: 1;
  min-height: 0;
  display: flex;
}

.home__list-pane {
  flex: none;
  width: 376px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--sy-border);
}

.home__detail {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: auto;
}

.home__detail > * {
  flex: 1;
  min-height: 0;
}

.home__detail-empty {
  align-self: center;
  justify-content: center;
  flex: 1;
}

/* Узкое окно: панели встают друг под друга, список перестаёт быть колонкой. */
@media (max-width: 860px) {
  .home__panes {
    flex-direction: column;
  }

  .home__list-pane {
    width: auto;
    border-right: none;
    border-bottom: 1px solid var(--sy-border);
  }
}

/* Панель поиска */

.home__toolbar {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
}

.home__toolbar-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
}

.home__new {
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

.home__new:hover {
  filter: brightness(1.08);
}

.home__new span,
.home__new span::before {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 13px;
  height: 1.5px;
  background: var(--sy-accent-fg);
  transform: translate(-50%, -50%);
  content: '';
}

.home__new span::before {
  transform: translate(-50%, -50%) rotate(90deg);
}

.home__search {
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

.home__search:focus-within {
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.home__search-icon {
  flex: none;
  width: 11px;
  height: 11px;
  border: 1.5px solid var(--sy-text-3);
  border-radius: 50%;
}

.home__search-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--sy-text);
  font-family: inherit;
  font-size: var(--sy-text-body);
}

.home__search-input::placeholder {
  color: var(--sy-text-3);
}

.home__search-input::-webkit-search-cancel-button {
  display: none;
}

.home__count {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.home__hotkey {
  flex: none;
  padding: 2px var(--sy-space-2);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-xs);
}

/* Список */

.home__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

/* Строка списка — кнопка: открывается и мышью, и с клавиатуры. */
.home__row {
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

.home__row:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.home__note {
  padding: var(--sy-space-8) var(--sy-space-6);
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
}

.home__note--error {
  color: var(--sy-danger);
}

.home__list {
  display: flex;
  flex-direction: column;
}

.home__group-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.home__group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6) var(--sy-space-2);
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.home__group-count {
  flex: none;
  color: var(--sy-accent);
}

/* Скелет загрузки */

.home__skeleton {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  padding: var(--sy-space-5) var(--sy-space-6);
}

.home__skeleton-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  height: 48px;
  padding: 0 var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  animation: home-shimmer 1.6s ease-in-out infinite;
}

.home__skeleton-row:nth-child(2) {
  animation-delay: 0.15s;
}

.home__skeleton-row:nth-child(3) {
  animation-delay: 0.3s;
}

.home__skeleton-row:nth-child(4) {
  animation-delay: 0.45s;
}

.home__skeleton-icon {
  flex: none;
  width: 28px;
  height: 28px;
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface-2);
}

.home__skeleton-lines {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.home__skeleton-line {
  width: 52%;
  height: 9px;
  border-radius: 3px;
  background: var(--sy-surface-2);
}

.home__skeleton-line--short {
  width: 38%;
  height: 8px;
}

@keyframes home-shimmer {
  0%,
  100% {
    opacity: 0.55;
  }
  50% {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .home__skeleton-row {
    animation: none;
  }
}
</style>
