<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import SectionEditor from '@/components/sections/SectionEditor.vue'
import { SyButton, SyModal, SyToggle, vaultColorVar } from '@/components/ui'
import { pluralize, RECORD_FORMS } from '@/composables/plural'
import { VAULT_COLORS, VAULT_NAME_MAX_LENGTH, type Vault, type VaultColor } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useToastStore } from '@/stores/useToastStore'

/**
 * Управление секциями (F7, §4.2 и экран «Секции» макета).
 *
 * Два обещания этого экрана, которые нельзя нарушать:
 *  1. Удаление секции НЕ удаляет записи — они переезжают в секцию по умолчанию.
 *     Поэтому кнопка удаления здесь обычная, а не красная: красным помечается
 *     только то, после чего данные правда теряются.
 *  2. Тумблер синхронизации (§4.2) — единственное, что секция решает всерьёз:
 *     уедут её записи на другие устройства или останутся здесь. Формулировки
 *     рядом с ним прямые, без «расширенных настроек».
 *
 * ЗАКОН №1: секретов на экране нет вообще — ни одной команды за ними отсюда не
 * уходит. Записи нужны только чтобы посчитать, сколько их в секции.
 */

const sections = useSectionsStore()
const list = useRecordsStore()
const toast = useToastStore()

onMounted(() => {
  void sections.load()
  // Счётчики секций считаются по метаданным записей — они и так в сторе, но на
  // этот экран можно прийти по прямой ссылке.
  void list.load()
})

function count(vault: Vault): number {
  return list.countByVault.get(vault.vault_id) ?? 0
}

/** Подпись под именем секции: сколько записей и куда они уезжают. */
function subtitle(vault: Vault): string {
  const records = pluralize(count(vault), RECORD_FORMS)
  const sync = vault.sync ? 'синхронизируется' : 'остаётся на этом устройстве'
  return vault.is_default ? `${records} · ${sync} · сюда попадают новые` : `${records} · ${sync}`
}

// ---------------------------------------------------------------------------
// Ошибки действий
//
// Отказ показываем у той секции, на которой нажали: экран целиком гасить
// незачем — остальные секции продолжают работать.
// ---------------------------------------------------------------------------

const actionError = ref<{ vaultId: string; message: string } | null>(null)
const busyId = ref<string | null>(null)

function fail(vaultId: string, cause: unknown, fallback: string): void {
  actionError.value = {
    vaultId,
    message: isCoreError(cause) ? cause.message : fallback,
  }
}

function errorFor(vaultId: string): string | null {
  return actionError.value?.vaultId === vaultId ? actionError.value.message : null
}

// ---------------------------------------------------------------------------
// Синхронизация секции (§4.2)
// ---------------------------------------------------------------------------

async function toggleSync(vault: Vault, sync: boolean): Promise<void> {
  actionError.value = null
  busyId.value = vault.vault_id
  try {
    await sections.setSync(vault.vault_id, sync)
    toast.push(
      sync
        ? `Секция «${vault.name}» синхронизируется`
        : `Секция «${vault.name}» останется на этом устройстве`,
      'neutral',
    )
  } catch (cause) {
    fail(vault.vault_id, cause, 'Не удалось изменить синхронизацию секции.')
  } finally {
    busyId.value = null
  }
}

async function makeDefault(vault: Vault): Promise<void> {
  actionError.value = null
  busyId.value = vault.vault_id
  try {
    await sections.setDefault(vault.vault_id)
    toast.push(`Новые записи будут попадать в «${vault.name}»`, 'neutral')
  } catch (cause) {
    fail(vault.vault_id, cause, 'Не удалось назначить секцию по умолчанию.')
  } finally {
    busyId.value = null
  }
}

// ---------------------------------------------------------------------------
// Создание и переименование
// ---------------------------------------------------------------------------

/** `'new'` — форма создания; id секции — форма её правки. Открыта всегда одна. */
const editing = ref<string | null>(null)
const draftName = ref('')
const draftColor = ref<VaultColor>('indigo')
const saving = ref(false)
const formError = ref<string | null>(null)

const nameError = computed(() => {
  const trimmed = draftName.value.trim()
  if (trimmed === '') return null
  return trimmed.length > VAULT_NAME_MAX_LENGTH
    ? `Не длиннее ${VAULT_NAME_MAX_LENGTH} символов.`
    : null
})

function startCreate(): void {
  editing.value = 'new'
  draftName.value = ''
  // Первый неиспользованный цвет: две одинаковые метки не различают ничего.
  draftColor.value =
    VAULT_COLORS.find((color) => !sections.vaults.some((vault) => vault.color === color)) ??
    VAULT_COLORS[0]
  formError.value = null
}

function startEdit(vault: Vault): void {
  editing.value = vault.vault_id
  draftName.value = vault.name
  draftColor.value = vault.color
  formError.value = null
}

function cancelEdit(): void {
  editing.value = null
  draftName.value = ''
  formError.value = null
}

async function submitDraft(): Promise<void> {
  if (saving.value || nameError.value !== null) return

  const name = draftName.value.trim()
  if (name === '') {
    formError.value = 'У секции должно быть имя.'
    return
  }

  saving.value = true
  formError.value = null
  try {
    if (editing.value === 'new') {
      await sections.create(name, draftColor.value)
      toast.push(`Секция «${name}» создана`, 'success')
    } else if (editing.value !== null) {
      await sections.update(editing.value, { name, color: draftColor.value })
      toast.push('Секция изменена', 'success')
    }
    cancelEdit()
  } catch (cause) {
    formError.value = isCoreError(cause) ? cause.message : 'Не удалось сохранить секцию.'
  } finally {
    saving.value = false
  }
}

// ---------------------------------------------------------------------------
// Удаление
// ---------------------------------------------------------------------------

const doomed = ref<Vault | null>(null)
const deleting = ref(false)
const deleteError = ref<string | null>(null)

function askDelete(vault: Vault): void {
  deleteError.value = null
  doomed.value = vault
}

const deleteText = computed(() => {
  const vault = doomed.value
  if (vault === null) return ''

  const moved = pluralize(count(vault), RECORD_FORMS)
  const home = sections.defaultVault?.name ?? 'секцию по умолчанию'
  return `Записи из неё не пропадут: ${moved} переедут в «${home}» и останутся на всех ваших устройствах. Сама секция исчезнет из сайдбара.`
})

async function confirmDelete(): Promise<void> {
  const vault = doomed.value
  if (vault === null) return

  deleting.value = true
  deleteError.value = null
  try {
    await sections.remove(vault.vault_id)
    // Записи сменили секцию в ядре — перечитываем, иначе список показывал бы
    // их в секции, которой больше нет.
    list.forgetVault(vault.vault_id)
    await list.load()
    doomed.value = null
    toast.push(`Секция «${vault.name}» удалена, записи на месте`, 'neutral')
  } catch (cause) {
    deleteError.value = isCoreError(cause) ? cause.message : 'Не удалось удалить секцию.'
  } finally {
    deleting.value = false
  }
}
</script>

<template>
  <main class="sections-view">
    <header class="sections-view__header">
      <div class="sections-view__intro">
        <h1 class="sections-view__title">Секции</h1>
        <p class="sections-view__lead">
          По умолчанию всё лежит в одном пространстве. Секция — это фильтр в сайдбаре, а не
          отдельное хранилище: ключ у всех секций один.
        </p>
      </div>
      <RouterLink class="sections-view__done" :to="{ name: 'home' }">Готово</RouterLink>
    </header>

    <div class="sections-view__body">
      <p v-if="sections.error" class="sections-view__error" role="alert">{{ sections.error }}</p>

      <div class="sections-view__cards">
        <template v-for="vault in sections.vaults" :key="vault.vault_id">
          <SectionEditor
            v-if="editing === vault.vault_id"
            v-model:name="draftName"
            v-model:color="draftColor"
            label="Название секции"
            submit-label="Сохранить"
            :saving="saving"
            :error="nameError ?? formError"
            @submit="submitDraft"
            @cancel="cancelEdit"
          />

          <div v-else class="sections-view__card">
            <span class="sections-view__badge" aria-hidden="true">
              <span :style="{ background: vaultColorVar(vault.color) }" />
            </span>

            <div class="sections-view__card-text">
              <div class="sections-view__card-title">
                <span class="sections-view__name">{{ vault.name }}</span>
                <span v-if="vault.is_default" class="sections-view__tag">по умолчанию</span>
              </div>
              <p class="sections-view__sub">{{ subtitle(vault) }}</p>
              <p v-if="errorFor(vault.vault_id)" class="sections-view__error" role="alert">
                {{ errorFor(vault.vault_id) }}
              </p>
            </div>

            <SyToggle
              class="sections-view__sync"
              :model-value="vault.sync"
              label="Синхронизировать"
              :state-text="vault.sync ? 'на всех устройствах' : 'только здесь'"
              :busy="busyId === vault.vault_id"
              @update:model-value="toggleSync(vault, $event)"
            />

            <div class="sections-view__card-actions">
              <SyButton
                v-if="!vault.is_default"
                size="sm"
                :disabled="busyId === vault.vault_id"
                @click="makeDefault(vault)"
              >
                Сюда по умолчанию
              </SyButton>
              <SyButton size="sm" @click="startEdit(vault)">Переименовать</SyButton>
              <SyButton
                v-if="!vault.is_default"
                size="sm"
                variant="ghost"
                @click="askDelete(vault)"
              >
                Удалить
              </SyButton>
            </div>
          </div>
        </template>

        <SectionEditor
          v-if="editing === 'new'"
          v-model:name="draftName"
          v-model:color="draftColor"
          label="Название новой секции"
          submit-label="Создать"
          :saving="saving"
          :error="nameError ?? formError"
          @submit="submitDraft"
          @cancel="cancelEdit"
        />

        <button
          v-else
          type="button"
          class="sections-view__new"
          :disabled="editing !== null"
          @click="startCreate"
        >
          Новая секция
        </button>
      </div>

      <div class="sections-view__notes">
        <section class="sections-view__note">
          <h2 class="sections-view__note-title">Удаление секции не удаляет записи</h2>
          <p class="sections-view__note-text">
            Записи вернутся в секцию по умолчанию и останутся на всех ваших устройствах. Красным
            помечаются только действия, после которых данные действительно теряются.
          </p>
        </section>
        <section class="sections-view__note">
          <h2 class="sections-view__note-title">Одна запись — одна секция</h2>
          <p class="sections-view__note-text">
            Секция выбирается в форме записи выпадающим списком. Новые секции появляются в нём
            сразу, без перезапуска.
          </p>
        </section>
        <section class="sections-view__note">
          <h2 class="sections-view__note-title">Локальная секция никуда не уезжает</h2>
          <p class="sections-view__note-text">
            С выключенной синхронизацией записи секции остаются на этом устройстве: их не будет на
            других, даже сопряжённых, и они не попадут в копию на них. Единственная копия таких
            записей — здесь.
          </p>
        </section>
      </div>
    </div>

    <SyModal
      :open="doomed !== null"
      :title="`Удалить секцию «${doomed?.name ?? ''}»?`"
      @close="doomed = null"
    >
      <p>{{ deleteText }}</p>
      <p v-if="doomed && !doomed.sync" class="sections-view__modal-note">
        Секция была локальной — после переезда её записи начнут синхронизироваться вместе с секцией
        по умолчанию.
      </p>
      <p v-if="deleteError" class="sections-view__error" role="alert">{{ deleteError }}</p>

      <template #actions>
        <SyButton size="sm" :disabled="deleting" @click="doomed = null">Отмена</SyButton>
        <SyButton size="sm" :loading="deleting" @click="confirmDelete">Удалить секцию</SyButton>
      </template>
    </SyModal>
  </main>
</template>

<style scoped>
/*
 * Экран — правая панель окна (F13), а не отдельная страница: он занимает
 * остаток ширины и высоту окна, а прокручивается его тело, оставляя сайдбар и
 * полосу заголовка на месте.
 */
.sections-view {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  background: var(--sy-bg-1);
}

.sections-view__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-8) var(--sy-space-9) var(--sy-space-7);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.sections-view__intro {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  max-width: 620px;
}

.sections-view__title {
  font-size: 22px;
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.sections-view__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.sections-view__done {
  flex: none;
  display: grid;
  place-items: center;
  height: var(--sy-control-height-sm);
  padding: 0 var(--sy-space-6);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
  color: var(--sy-text);
  text-decoration: none;
}

.sections-view__done:hover {
  background: var(--sy-surface-2);
}

.sections-view__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-8);
  padding: var(--sy-space-8) var(--sy-space-9) var(--sy-space-9);
}

/* Карточки секций не сжимаются — тело прокручивается (см. `DevicesView`). */
.sections-view__body > * {
  flex: none;
}

.sections-view__cards {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  max-width: 880px;
}

.sections-view__card {
  display: flex;
  align-items: center;
  gap: var(--sy-space-6);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.sections-view__badge {
  flex: none;
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border: 1px solid var(--sy-border-strong);
  border-radius: 9px;
  background: var(--sy-bg-1);
}

.sections-view__badge span {
  width: 10px;
  height: 10px;
  border-radius: 3px;
}

.sections-view__card-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.sections-view__card-title {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
}

.sections-view__name {
  font-size: 14.5px;
  font-weight: var(--sy-weight-medium);
}

.sections-view__tag {
  flex: none;
  padding: 2px 7px;
  border: 1px solid var(--sy-accent-border);
  border-radius: 5px;
  background: var(--sy-accent-quiet);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  color: var(--sy-accent);
}

.sections-view__sub {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.sections-view__sync {
  flex: none;
  width: 210px;
}

.sections-view__card-actions {
  flex: none;
  display: flex;
  gap: var(--sy-space-3);
}

.sections-view__new {
  height: 44px;
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius);
  background: transparent;
  font-family: inherit;
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.sections-view__new:hover:not(:disabled) {
  border-color: var(--sy-accent);
  color: var(--sy-text);
}

.sections-view__new:disabled {
  color: var(--sy-text-3);
  cursor: not-allowed;
}

/* Пояснения */

.sections-view__notes {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--sy-space-6);
  max-width: 880px;
}

.sections-view__note {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
}

.sections-view__note-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.sections-view__note-text {
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.sections-view__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

.sections-view__modal-note {
  margin-top: var(--sy-space-3);
  color: var(--sy-text-3);
}
</style>
