<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import ConflictBanner from '@/components/conflicts/ConflictBanner.vue'
import ConflictDialog from '@/components/conflicts/ConflictDialog.vue'
import {
  iconHue,
  iconInitials,
  SyButton,
  SyCopyButton,
  SyModal,
  SySecretField,
  vaultColorVar,
} from '@/components/ui'
import { securityPolicy } from '@/composables/securityPolicy'
import { useRecordSecrets } from '@/composables/useRecordSecrets'
import type { RecordMeta } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useSyncStore } from '@/stores/useSyncStore'
import { useToastStore } from '@/stores/useToastStore'

import { formatDate, passwordAgeWarning } from './recordFormat'

/**
 * Карточка записи (F5, §3.3).
 *
 * Макет делит экран на два блока — «метаданные · видны сразу» и «секреты ·
 * открываются явно», — и это не украшение: граница между тем, что уже на
 * экране, и тем, что появится только по нажатию, должна читаться с одного
 * взгляда.
 *
 * ЗАКОН №1: карточка не держит секретов. Всё, что она знает о них из
 * метаданных, — заполнены ли поля (`has_notes`, `has_totp`). Значения живут в
 * `useRecordSecrets` ровно пока открыты.
 */

const props = defineProps<{ record: RecordMeta }>()
const emit = defineEmits<{ edit: [] }>()

const list = useRecordsStore()
const sections = useSectionsStore()
const conflicts = useConflictsStore()
const sync = useSyncStore()
const toast = useToastStore()

onMounted(() => {
  void sections.ensure()
  void conflicts.ensure()
  // Подвал говорит, уехала ли запись; дальше состояние приезжает событиями.
  void sync.ensure()
})

/**
 * Конфликт версий этой записи (F11). Полоса стоит НАД карточкой: пока выбор не
 * сделан, показанный ниже пароль — только одна из двух правд, и узнать об этом
 * человек должен раньше, чем поверит увиденному.
 */
const conflict = computed(() => conflicts.byRecord(props.record.record_id))
const resolving = ref(false)

/** Изменение сделано здесь и ещё не уехало (F10). */
const isPending = computed(() => sync.isPending(props.record.record_id))

/**
 * Секция записи (F7). Синхронизация настраивается по секциям (§4.2), поэтому
 * ответ на вопрос «а этот пароль вообще есть на других устройствах?» должен
 * быть виден прямо здесь, а не только в настройках.
 */
const section = computed(() => sections.byId(props.record.vault_id))

const recordId = computed(() => props.record.record_id)
const secrets = useRecordSecrets(recordId)
const clipboard = secrets.clipboard

const hue = computed(() => iconHue(props.record.urls[0] ?? props.record.service_name))
const initials = computed(() => iconInitials(props.record.service_name))
const ageWarning = computed(() => passwordAgeWarning(props.record.password_updated_at))

/** Копирование метаданных: буфер не чистим — там нет секрета. */
async function copyMeta(key: string, value: string, what: string): Promise<void> {
  if (await clipboard.copy(key, value)) toast.push(`${what} скопирован`, 'success')
  else toast.push('Буфер обмена недоступен', 'danger')
}

async function copySecret(field: 'password' | 'notes' | 'totp_secret'): Promise<void> {
  const done = await secrets.copy(field)
  if (done) {
    // Срок берём из действующей политики, а не из константы в тексте: с F13 его
    // выбирает пользователь, и зашитое «20 с» стало бы враньём при первом же
    // изменении настройки.
    const seconds = Math.ceil(securityPolicy().value.clipboard_clear_ms / 1000)
    toast.push(`Пароль в буфере · очистится через ${seconds} с`, 'success')
  } else if (secrets.error.value === null) {
    toast.push('Буфер обмена недоступен', 'danger')
  }
}

// ---------------------------------------------------------------------------
// Удаление
// ---------------------------------------------------------------------------

const confirming = ref(false)
const deleting = ref(false)
const deleteError = ref<string | null>(null)

function askDelete(): void {
  deleteError.value = null
  confirming.value = true
}

async function confirmDelete(): Promise<void> {
  deleting.value = true
  deleteError.value = null
  try {
    await list.remove(props.record.record_id)
    confirming.value = false
    toast.push('Запись удалена', 'neutral')
  } catch (cause) {
    deleteError.value = isCoreError(cause) ? cause.message : 'Не удалось удалить запись.'
  } finally {
    deleting.value = false
  }
}

// Заголовок берёт настоящее имя записи: подтверждать удаление «чего-то»
// пользователь не должен.
const deleteTitle = computed(() => `Удалить «${props.record.service_name}»?`)
</script>

<template>
  <article class="card">
    <header class="card__head">
      <span
        class="card__icon"
        :style="{
          background: `oklch(var(--sy-icon-bg) ${hue})`,
          borderColor: `oklch(var(--sy-icon-border) ${hue})`,
          color: `oklch(var(--sy-icon-ink) ${hue})`,
        }"
        aria-hidden="true"
        >{{ initials }}</span
      >

      <div class="card__ident">
        <div class="card__title-row">
          <h2 class="card__title">{{ record.service_name }}</h2>
          <span v-if="record.account_label" class="card__label">{{ record.account_label }}</span>
        </div>
        <p class="card__login">{{ record.login }}</p>
      </div>

      <div class="card__head-actions">
        <SyButton size="sm" @click="emit('edit')">Изменить</SyButton>
      </div>
    </header>

    <div class="card__body">
      <ConflictBanner
        v-if="conflict"
        :device-name="conflict.remote.device_name"
        @open="resolving = true"
      />

      <section class="card__block">
        <div class="card__block-head">
          <span class="card__block-title">Метаданные · видны сразу</span>
          <span class="card__rule" aria-hidden="true" />
        </div>

        <div class="card__grid">
          <div class="card__field">
            <span class="card__field-label">Адреса сайта</span>
            <div v-if="record.urls.length === 0" class="card__value card__value--empty">
              Адресов нет — автозаполнению не за что зацепиться
            </div>
            <div v-for="(url, index) in record.urls" :key="url" class="card__value">
              <span class="card__value-text">{{ url }}</span>
              <SyCopyButton
                compact
                label="Скопировать адрес"
                :copied="clipboard.copiedKey.value === `url:${index}`"
                :unavailable="!clipboard.available.value"
                @click="copyMeta(`url:${index}`, url, 'Адрес')"
              />
            </div>
          </div>

          <div class="card__field">
            <span class="card__field-label">Логин</span>
            <div class="card__value">
              <span class="card__value-text">{{ record.login }}</span>
              <SyCopyButton
                compact
                label="Скопировать логин"
                :copied="clipboard.copiedKey.value === 'login'"
                :unavailable="!clipboard.available.value"
                @click="copyMeta('login', record.login, 'Логин')"
              />
            </div>
          </div>

          <div class="card__field">
            <span class="card__field-label">Секция</span>
            <div class="card__value card__value--plain">
              <span
                v-if="section"
                class="card__section-dot"
                :style="{ background: vaultColorVar(section.color) }"
                aria-hidden="true"
              />
              <span class="card__value-text">{{ section?.name ?? '—' }}</span>
              <span v-if="section" class="card__section-sync">
                {{ section.sync ? 'синхронизируется' : 'только это устройство' }}
              </span>
            </div>
          </div>

          <div class="card__field">
            <span class="card__field-label">Создано · пароль изменён</span>
            <div class="card__dates">
              <span>{{ formatDate(record.created_at) }}</span>
              <span class="card__dates-sep" aria-hidden="true" />
              <span>{{ formatDate(record.password_updated_at) }}</span>
            </div>
          </div>

          <div class="card__field">
            <span class="card__field-label">Версия</span>
            <div class="card__dates">
              <span>№ {{ record.version }}</span>
              <span class="card__dates-sep" aria-hidden="true" />
              <span>изменена {{ formatDate(record.updated_at) }}</span>
            </div>
          </div>
        </div>
      </section>

      <section class="card__block">
        <div class="card__block-head">
          <span class="card__block-title card__block-title--accent"
            >Секреты · открываются явно</span
          >
          <span class="card__rule" aria-hidden="true" />
          <span class="card__block-note">Копировать можно, не открывая</span>
        </div>

        <p v-if="secrets.error.value" class="card__error" role="alert">{{ secrets.error.value }}</p>

        <SySecretField
          label="Пароль"
          :value="secrets.shown.password"
          :hide-in="secrets.hideIn.password"
          :busy="secrets.busy.value === 'password'"
          :copied="clipboard.copiedKey.value === 'password'"
          :copy-seconds="clipboard.secondsLeft.value"
          :clipboard-unavailable="!clipboard.available.value"
          @toggle="secrets.toggle('password')"
          @copy="copySecret('password')"
        />

        <div v-if="ageWarning" class="card__age" role="status">
          <span class="card__age-dot" aria-hidden="true" />
          <span>{{ ageWarning }}</span>
        </div>

        <div class="card__grid card__grid--secrets">
          <SySecretField
            label="Ключ TOTP"
            :value="secrets.shown.totp_secret"
            :present="record.has_totp"
            empty-text="Не подключён"
            hint="Ключ хранится и синхронизируется · сами коды — в следующей версии"
            :hide-in="secrets.hideIn.totp_secret"
            :busy="secrets.busy.value === 'totp_secret'"
            :copied="clipboard.copiedKey.value === 'totp_secret'"
            :copy-seconds="clipboard.secondsLeft.value"
            :clipboard-unavailable="!clipboard.available.value"
            @toggle="secrets.toggle('totp_secret')"
            @copy="copySecret('totp_secret')"
          />

          <SySecretField
            label="Заметки"
            multiline
            :value="secrets.shown.notes"
            :present="record.has_notes"
            empty-text="Заметок нет"
            :hide-in="secrets.hideIn.notes"
            :busy="secrets.busy.value === 'notes'"
            :copied="clipboard.copiedKey.value === 'notes'"
            :copy-seconds="clipboard.secondsLeft.value"
            :clipboard-unavailable="!clipboard.available.value"
            @toggle="secrets.toggle('notes')"
            @copy="copySecret('notes')"
          />
        </div>
      </section>
    </div>

    <footer class="card__foot">
      <span class="card__foot-note">
        {{
          section !== null && !section.sync
            ? `Секция «${section.name}» локальная · копии этой записи на других устройствах нет`
            : isPending
              ? 'Изменение сохранено здесь и уедет, когда рядом окажется другое устройство'
              : 'Хранится на этом устройстве · копии есть на сопряжённых устройствах'
        }}
      </span>
      <SyButton variant="danger" size="sm" @click="askDelete">Удалить запись</SyButton>
    </footer>

    <SyModal
      :open="confirming"
      :title="deleteTitle"
      tone="danger"
      :warning="
        record.has_totp
          ? 'Вместе с записью пропадёт и её код подтверждения — без него вход в сервис придётся восстанавливать.'
          : undefined
      "
      @close="confirming = false"
    >
      <p class="card__confirm-meta">
        {{ record.login }} · изменена {{ formatDate(record.updated_at) }}
      </p>
      <p>
        Запись и её пароль удалятся с этого устройства и со всех остальных при следующей
        синхронизации. Восстановить её из Syncra будет нельзя.
      </p>
      <p v-if="deleteError" class="card__error" role="alert">{{ deleteError }}</p>

      <template #actions>
        <span class="card__confirm-hint">Подтвердите повторным нажатием «Удалить запись»</span>
        <SyButton size="sm" :disabled="deleting" @click="confirming = false">Отмена</SyButton>
        <SyButton variant="danger" size="sm" :loading="deleting" @click="confirmDelete">
          Удалить запись
        </SyButton>
      </template>
    </SyModal>

    <ConflictDialog v-if="conflict && resolving" :conflict="conflict" @close="resolving = false" />
  </article>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

/* Шапка */

.card__head {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-6);
  padding: var(--sy-space-7) var(--sy-space-8) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
}

.card__icon {
  flex: none;
  display: grid;
  place-items: center;
  width: 46px;
  height: 46px;
  border: 1px solid transparent;
  border-radius: 13px;
  font-family: var(--sy-font-mono);
  font-size: 17px;
  font-weight: var(--sy-weight-semibold);
}

.card__ident {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.card__title-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
}

.card__title {
  font-size: 22px;
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card__label {
  flex: none;
  padding: 2px 7px;
  border: 1px solid var(--sy-border-strong);
  border-radius: 5px;
  font-family: var(--sy-font-mono);
  font-size: 10px;
  color: var(--sy-text-2);
}

.card__login {
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
}

.card__head-actions {
  flex: none;
  display: flex;
  gap: var(--sy-space-3);
}

/* Тело */

.card__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-8);
  padding: var(--sy-space-7) var(--sy-space-8) var(--sy-space-8);
}

.card__block {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
}

.card__block-head {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.card__block-title {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.card__block-title--accent {
  color: var(--sy-accent);
}

.card__block-note {
  flex: none;
  font-size: 11.5px;
  color: var(--sy-text-3);
}

.card__rule {
  flex: 1;
  height: 1px;
  background: var(--sy-border);
}

.card__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--sy-space-6) var(--sy-space-7);
}

.card__grid--secrets {
  align-items: start;
}

.card__field {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
  min-width: 0;
}

.card__field-label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.card__value {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  height: 38px;
  padding: 0 var(--sy-space-2) 0 var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
}

.card__value + .card__value {
  margin-top: var(--sy-space-2);
}

/* Значение, которое нечего копировать: рамка есть, кнопки нет. */
.card__value--plain {
  padding-right: var(--sy-space-5);
}

.card__section-dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 2px;
}

.card__section-sync {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 10px;
  color: var(--sy-text-3);
}

.card__value--empty {
  border-style: dashed;
  background: transparent;
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
}

.card__value-text {
  flex: 1;
  min-width: 0;
  font-size: var(--sy-text-body);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card__dates {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  height: 38px;
  font-family: var(--sy-font-mono);
  font-size: 12px;
  color: var(--sy-text-2);
}

.card__dates-sep {
  width: 1px;
  height: 14px;
  background: var(--sy-border);
}

.card__age {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  padding: 9px var(--sy-space-5);
  border: 1px solid var(--sy-warn);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-warn-quiet);
  font-size: var(--sy-text-small);
  color: var(--sy-text);
}

.card__age-dot {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-warn);
}

.card__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

/* Подвал */

.card__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-8);
  border-top: 1px solid var(--sy-border);
}

.card__foot-note {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.card__confirm-meta {
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-3);
  margin-bottom: var(--sy-space-4);
}

.card__confirm-hint {
  flex: 1;
  font-size: 12px;
  line-height: 1.4;
  color: var(--sy-text-3);
}
</style>
