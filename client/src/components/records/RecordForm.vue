<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'

import PasswordGenerator from '@/components/generator/PasswordGenerator.vue'
import { SyButton, SyInput, SySelect } from '@/components/ui'
import { normalizeHost } from '@/composables/useRecordList'
import type { RecordDraft, RecordMeta, RecordPatch, SecretField, VaultId } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore } from '@/core/ipc'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useToastStore } from '@/stores/useToastStore'

/**
 * Форма создания и редактирования записи (F5, §3.4 макета).
 *
 * ЗАКОН №1 и редактирование секретов. Форма НЕ подгружает пароль, заметки и
 * ключ TOTP автоматически: открытая карточка на редактирование не повод
 * выкладывать секреты на экран. Пустое секретное поле означает «оставить как
 * было» — патч его просто не содержит. Захотел заменить — набери новое;
 * захотел дописать к заметке — нажми «Показать текущее», и это будет такой же
 * явный разовый reveal, как в карточке.
 *
 * Черновик с секретами живёт в локальном состоянии формы, уходит в ядро одной
 * командой и исчезает вместе с компонентом. В Pinia он не попадает.
 *
 * Секция (F7, §4.2) выбирается здесь же: одна запись живёт ровно в одной
 * секции, и это единственное место, где её переносят.
 */

const props = defineProps<{ record?: RecordMeta | null }>()
const emit = defineEmits<{ saved: [record: RecordMeta]; cancel: [] }>()

const list = useRecordsStore()
const sections = useSectionsStore()
const toast = useToastStore()

const isEdit = computed(() => props.record != null)

// ---------------------------------------------------------------------------
// Метаданные
// ---------------------------------------------------------------------------

const meta = reactive({
  service_name: props.record?.service_name ?? '',
  login: props.record?.login ?? '',
  account_label: props.record?.account_label ?? '',
})

/**
 * Секция записи (F7). У новой записи предлагается та, что открыта в сайдбаре:
 * человек только что смотрел «Рабочее» и жмёт «новая запись» — он заводит
 * рабочую. Если фильтра нет, решает ядро (секция по умолчанию), и пустая
 * строка означает именно это.
 */
const vaultChoice = ref<VaultId | null>(props.record?.vault_id ?? list.vaultFilter ?? null)

/** Что выбрано в списке: явный выбор или — пока его нет — секция по умолчанию. */
const shownVault = computed(() => vaultChoice.value ?? sections.defaultVault?.vault_id ?? '')

function chooseVault(next: string): void {
  vaultChoice.value = next === '' ? null : next
}

const vaultOptions = computed(() => {
  const options = sections.vaults.map((vault) => ({
    value: vault.vault_id,
    label: vault.sync ? vault.name : `${vault.name} · локальная`,
  }))
  // Пока ядро не ответило, список не должен молча подменять секцию записи
  // первой попавшейся строкой.
  if (shownVault.value !== '' && !options.some((option) => option.value === shownVault.value)) {
    options.unshift({ value: shownVault.value, label: 'Текущая секция' })
  }
  return options
})

const selectedVault = computed(() =>
  sections.byId(shownVault.value === '' ? null : shownVault.value),
)

const vaultHint = computed(() => {
  const vault = selectedVault.value
  if (vault === null) return 'Ляжет в секцию по умолчанию.'
  return vault.sync
    ? 'Запись уедет на другие устройства вместе с секцией.'
    : 'Секция локальная — запись останется на этом устройстве.'
})

onMounted(() => {
  void sections.ensure()
})

/** Всегда есть хотя бы одна строка адреса — иначе не за что нажать. */
const urls = ref<string[]>(props.record?.urls.length ? [...props.record.urls] : [''])

function addUrl(): void {
  urls.value = [...urls.value, '']
}

function removeUrl(index: number): void {
  const next = urls.value.filter((_, i) => i !== index)
  urls.value = next.length > 0 ? next : ['']
}

function setUrl(index: number, value: string): void {
  urls.value = urls.value.map((url, i) => (i === index ? value : url))
}

/**
 * Мягкая проверка адреса: он нужен для матчинга автозаполнения, поэтому в нём
 * должен быть хост. Схема, путь и порт необязательны — пользователь вправе
 * написать просто `github.com`.
 */
function urlError(value: string): string | null {
  const trimmed = value.trim()
  if (trimmed === '') return null

  const host = normalizeHost(trimmed)
  if (host === null || /\s/.test(host) || !host.includes('.')) {
    return 'Не похоже на адрес. Пример: https://github.com'
  }
  return null
}

// ---------------------------------------------------------------------------
// Секреты
// ---------------------------------------------------------------------------

interface SecretDraft {
  value: string
  /** Что лежит в ядре сейчас. `null` — не запрашивали (и не будем без спроса). */
  original: string | null
  loading: boolean
}

function emptyDraft(): SecretDraft {
  return { value: '', original: null, loading: false }
}

const secrets = reactive<Record<SecretField, SecretDraft>>({
  password: emptyDraft(),
  notes: emptyDraft(),
  totp_secret: emptyDraft(),
})

const secretError = ref<string | null>(null)

/** Явный разовый reveal ради редактирования: то же действие, что в карточке. */
async function loadCurrent(field: SecretField): Promise<void> {
  const id = props.record?.record_id
  if (id === undefined) return

  secrets[field].loading = true
  secretError.value = null
  try {
    const value = (await useCore().getSecret(id))[field] ?? ''
    secrets[field].value = value
    secrets[field].original = value
  } catch (cause) {
    secretError.value = isCoreError(cause) ? cause.message : 'Не удалось получить секрет из ядра.'
  } finally {
    secrets[field].loading = false
  }
}

/** Изменилось ли поле настолько, чтобы отправлять его в ядро. */
function changed(field: SecretField): boolean {
  const draft = secrets[field]
  return draft.original === null ? draft.value !== '' : draft.value !== draft.original
}

function secretHint(field: SecretField, whenEmpty: string): string {
  if (!isEdit.value) return whenEmpty
  if (secrets[field].original !== null) return 'Текущее значение открыто — правьте и сохраняйте'
  return 'Пусто — останется как было'
}

// ---------------------------------------------------------------------------
// Отправка
// ---------------------------------------------------------------------------

const errors = reactive<Record<string, string | null>>({
  service_name: null,
  login: null,
  password: null,
})

const saving = ref(false)
const formError = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Генератор (F6)
// ---------------------------------------------------------------------------

/**
 * В новой записи панель открыта сразу: §6.1 обещает, что правила настраиваются
 * один раз, а дальше «в форме будет просто готовый пароль и кнопка „другой“».
 * Свежесгенерированная строка ещё ничей секрет — показать её не значит что-то
 * раскрыть.
 *
 * В редактировании панель закрыта: смена работающего пароля — осознанное
 * действие, и подсовывать замену тому, кто зашёл поправить логин, незачем.
 */
const generatorOpen = ref(!isEdit.value)

/** Выбранный вариант попадает в поле пароля — то есть в тот же черновик, что и ручной ввод. */
function usePassword(password: string): void {
  secrets.password.value = password
  errors.password = null
}

function validate(): boolean {
  errors.service_name =
    meta.service_name.trim() === '' ? 'Без имени сервиса запись не найти.' : null
  errors.login = meta.login.trim() === '' ? 'Логин обязателен.' : null

  if (!isEdit.value) {
    errors.password = secrets.password.value === '' ? 'Пароль обязателен.' : null
  } else {
    // В редактировании пустое поле значит «не менять». Пустым его можно
    // сделать, только сначала открыв текущее, — вот это и запрещаем.
    errors.password =
      secrets.password.original !== null && secrets.password.value === ''
        ? 'Пароль не может быть пустым.'
        : null
  }

  const badUrl = urls.value.some((url) => urlError(url) !== null)
  return !badUrl && Object.values(errors).every((error) => error === null)
}

/** Пустые строки адресов — не адреса, а следы кнопки «добавить». */
function cleanUrls(): string[] {
  return urls.value.map((url) => url.trim()).filter((url) => url !== '')
}

function label(): string | null {
  const trimmed = meta.account_label.trim()
  return trimmed === '' ? null : trimmed
}

async function save(): Promise<void> {
  if (saving.value || !validate()) return

  saving.value = true
  formError.value = null
  try {
    if (props.record == null) {
      const draft: RecordDraft = {
        // Секцию не указываем, если пользователь её не выбирал: пусть ядро
        // положит запись в свою секцию по умолчанию.
        vault_id: vaultChoice.value ?? undefined,
        service_name: meta.service_name.trim(),
        urls: cleanUrls(),
        login: meta.login.trim(),
        account_label: label(),
        password: secrets.password.value,
        notes: secrets.notes.value === '' ? null : secrets.notes.value,
        totp_secret: secrets.totp_secret.value === '' ? null : secrets.totp_secret.value,
      }
      const created = await list.create(draft)
      toast.push('Запись создана', 'success')
      emit('saved', created)
    } else {
      const patch: RecordPatch = {
        service_name: meta.service_name.trim(),
        urls: cleanUrls(),
        login: meta.login.trim(),
        account_label: label(),
      }
      // Переезд в другую секцию — тоже изменение записи, но только если её
      // правда переставили.
      if (vaultChoice.value !== null && vaultChoice.value !== props.record.vault_id) {
        patch.vault_id = vaultChoice.value
      }
      // Секретные поля попадают в патч, только если пользователь их правда
      // тронул: иначе каждое сохранение переписывало бы пароль сам собой.
      if (changed('password')) patch.password = secrets.password.value
      if (changed('notes')) patch.notes = secrets.notes.value === '' ? null : secrets.notes.value
      if (changed('totp_secret')) {
        patch.totp_secret = secrets.totp_secret.value === '' ? null : secrets.totp_secret.value
      }

      const updated = await list.update(props.record.record_id, patch)
      toast.push('Изменения сохранены', 'success')
      emit('saved', updated)
    }
  } catch (cause) {
    formError.value = isCoreError(cause) ? cause.message : 'Не удалось сохранить запись.'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <form class="form" @submit.prevent="save" @keydown.escape="emit('cancel')">
    <header class="form__head">
      <div class="form__head-text">
        <h2 class="form__title">{{ isEdit ? 'Изменить запись' : 'Новая запись' }}</h2>
        <p class="form__subtitle">
          {{
            isEdit
              ? 'Секретные поля пусты не случайно: пустое — значит «оставить как было».'
              : 'Черновик не покидает это окно, пока вы не сохраните. Ничего не уходит в сеть.'
          }}
        </p>
      </div>

      <div class="form__head-actions">
        <SyButton size="sm" :disabled="saving" @click="emit('cancel')">Отмена</SyButton>
        <SyButton variant="primary" size="sm" type="submit" :loading="saving">Сохранить</SyButton>
      </div>
    </header>

    <div class="form__body">
      <p v-if="formError" class="form__error" role="alert">{{ formError }}</p>

      <div class="form__grid">
        <SyInput
          v-model="meta.service_name"
          label="Имя сервиса"
          placeholder="Например, github.com"
          :error="errors.service_name"
          hint="Иконка соберётся из имени — из сети ничего не загружается."
          autofocus
          @submit="save"
        />

        <SyInput
          v-model="meta.login"
          label="Логин"
          placeholder="anna@example.com"
          :error="errors.login"
          autocomplete="off"
          @submit="save"
        />

        <div class="form__urls">
          <span class="form__label">Адреса сайта</span>
          <div v-for="(url, index) in urls" :key="index" class="form__url-row">
            <SyInput
              class="form__url-input"
              :model-value="url"
              type="url"
              placeholder="https://github.com"
              :error="urlError(url)"
              @update:model-value="setUrl(index, $event)"
              @submit="save"
            />
            <SyButton
              v-if="urls.length > 1 || url !== ''"
              size="sm"
              class="form__url-remove"
              @click="removeUrl(index)"
              >Убрать</SyButton
            >
          </div>
          <button type="button" class="form__add" @click="addUrl">Добавить ещё адрес</button>
        </div>

        <SyInput
          v-model="meta.account_label"
          label="Метка аккаунта"
          placeholder="личный / рабочий"
          hint="Ею вы отличите несколько аккаунтов одного сервиса."
          @submit="save"
        />

        <SySelect
          class="form__vault"
          :model-value="shownVault"
          label="Секция"
          :options="vaultOptions"
          :hint="vaultHint"
          @update:model-value="chooseVault"
        />
      </div>

      <section class="form__secrets">
        <div class="form__secrets-head">
          <span class="form__secrets-title">Секреты · шифруются ядром</span>
          <span class="form__rule" aria-hidden="true" />
        </div>

        <p v-if="secretError" class="form__error" role="alert">{{ secretError }}</p>

        <div class="form__secret">
          <SyInput
            v-model="secrets.password.value"
            label="Пароль"
            type="password"
            :placeholder="isEdit ? 'Оставить как было' : 'Введите, вставьте или подберите'"
            :error="errors.password"
            :hint="secretHint('password', 'Можно ввести свой или выбрать вариант генератора.')"
            autocomplete="new-password"
            @submit="save"
          />
          <SyButton
            v-if="!generatorOpen"
            size="sm"
            :aria-expanded="false"
            @click="generatorOpen = true"
            >Подобрать</SyButton
          >
          <SyButton
            v-if="isEdit && secrets.password.original === null"
            size="sm"
            :loading="secrets.password.loading"
            @click="loadCurrent('password')"
            >Показать текущий</SyButton
          >
        </div>

        <!--
          Панель генератора (F6). Живёт внутри блока секретов: варианты — это
          пароли, пусть пока и ничьи. `v-if`, а не `v-show`: закрытая панель
          должна быть размонтирована, чтобы её composable забыл варианты.
        -->
        <PasswordGenerator
          v-if="generatorOpen"
          @pick="usePassword"
          @close="generatorOpen = false"
        />

        <div class="form__secret form__secret--notes">
          <div class="form__field">
            <span class="form__label">Заметки · необязательно</span>
            <textarea
              v-model="secrets.notes.value"
              class="form__textarea"
              rows="3"
              :placeholder="
                isEdit ? 'Оставить как было' : 'Контрольные вопросы, коды восстановления, PIN'
              "
              spellcheck="false"
            />
            <p class="form__note">
              {{ secretHint('notes', 'Хранятся так же, как пароль: скрыты по умолчанию.') }}
            </p>
          </div>
          <SyButton
            v-if="isEdit && secrets.notes.original === null && record?.has_notes"
            size="sm"
            :loading="secrets.notes.loading"
            @click="loadCurrent('notes')"
            >Показать текущие</SyButton
          >
        </div>

        <div class="form__secret">
          <SyInput
            v-model="secrets.totp_secret.value"
            label="Ключ TOTP · необязательно"
            mono
            :placeholder="isEdit ? 'Оставить как было' : 'Ключ из настроек двухфакторной защиты'"
            :hint="
              secretHint(
                'totp_secret',
                'Ключ сохраним и синхронизируем · генерация кодов — в следующей версии.',
              )
            "
            @submit="save"
          />
          <SyButton
            v-if="isEdit && secrets.totp_secret.original === null && record?.has_totp"
            size="sm"
            :loading="secrets.totp_secret.loading"
            @click="loadCurrent('totp_secret')"
            >Показать текущий</SyButton
          >
        </div>
      </section>
    </div>
  </form>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

.form__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-7) var(--sy-space-8) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
}

.form__head-text {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  min-width: 0;
}

.form__title {
  font-size: 22px;
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.form__subtitle {
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.form__head-actions {
  flex: none;
  display: flex;
  gap: var(--sy-space-3);
}

.form__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-8);
  padding: var(--sy-space-7) var(--sy-space-8) var(--sy-space-8);
}

.form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--sy-space-6) var(--sy-space-7);
  align-items: start;
}

.form__field {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.form__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.form__note {
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--sy-text-3);
}

.form__urls {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  min-width: 0;
}

.form__url-row {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-3);
}

.form__url-input {
  flex: 1;
  min-width: 0;
}

.form__url-remove {
  flex: none;
  margin-top: 0;
}

.form__add {
  align-self: flex-start;
  border: none;
  background: none;
  padding: 0;
  color: var(--sy-accent);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}

.form__add:hover {
  text-decoration: underline;
}

.form__secrets {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
}

.form__secrets-head {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.form__secrets-title {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-accent);
}

.form__rule {
  flex: 1;
  height: 1px;
  background: var(--sy-border);
}

.form__secret {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-4);
}

.form__secret > :first-child {
  flex: 1;
  min-width: 0;
}

.form__secret .sy-button {
  margin-top: 22px;
}

.form__textarea {
  width: 100%;
  min-height: 76px;
  padding: var(--sy-space-4) var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  color: var(--sy-text);
  font-family: var(--sy-font-sans);
  font-size: 13.5px;
  line-height: 1.5;
  resize: vertical;
  outline: none;
  transition:
    border-color var(--sy-transition),
    box-shadow var(--sy-transition);
}

.form__textarea:focus {
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.form__textarea::placeholder {
  color: var(--sy-text-3);
}

.form__error {
  font-size: var(--sy-text-body);
  color: var(--sy-danger);
}
</style>
