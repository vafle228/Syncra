<script setup lang="ts">
import { computed, onMounted, ref, toRef } from 'vue'

import { formatDateTime } from '@/components/records/recordFormat'
import { SyButton, SyModal } from '@/components/ui'
import { useConflictSecrets } from '@/composables/useConflictSecrets'
import type { ConflictField, ConflictSide, ConflictVersion, RecordConflict } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useToastStore } from '@/stores/useToastStore'

import {
  CONFLICT_FIELD_ORDER,
  conflictFieldLabel,
  conflictMetaValue,
  differingLine,
  hasSecret,
  isSecretField,
  laterSide,
} from './conflictFormat'

/**
 * Разрешение конфликта версий (F11, §5.5).
 *
 * Три правила спека, которые здесь и держатся:
 *  1. Выбирается ВЕРСИЯ ЦЕЛИКОМ, а не поля по одному. Поэтому переключатель
 *     один на колонку, и колонка подсвечивается вся: человек выбирает «эту
 *     запись», а не собирает третью из двух.
 *  2. Система не угадывает. Ни одна версия не выбрана заранее — даже более
 *     свежая: позже не значит нужнее. Пока выбора нет, кнопка недоступна.
 *  3. Видно, какая версия раньше, какая позже — с точностью до минуты: обе
 *     правки часто сделаны в один день.
 *
 * ЗАКОН №1: значений секретов в конфликте нет. Видно, что пароли различаются
 * (это метаданное), а сами пароли открываются отдельным разовым запросом по
 * нажатию и гаснут сами — как и везде.
 */

const props = defineProps<{ conflict: RecordConflict }>()
const emit = defineEmits<{ close: [] }>()

const conflicts = useConflictsStore()
const sections = useSectionsStore()
const toast = useToastStore()

const recordId = toRef(() => props.conflict.record_id)
const secrets = useConflictSecrets(recordId)

onMounted(() => {
  void sections.ensure()
})

/** Ничего не выбрано заранее: систему не просили решать за человека. */
const side = ref<ConflictSide | null>(null)
const saving = ref(false)
const error = ref<string | null>(null)

const later = computed(() => laterSide(props.conflict))
const rows = computed(() => CONFLICT_FIELD_ORDER)

/** Маска той же длины, что и в карточке: длина пароля — тоже сведения о нём. */
const MASK = '••••••••••'

function versionOf(which: ConflictSide): ConflictVersion {
  return which === 'local' ? props.conflict.local : props.conflict.remote
}

function differs(field: ConflictField): boolean {
  return props.conflict.differing_fields.includes(field)
}

function cellValue(which: ConflictSide, field: ConflictField): string {
  const version = versionOf(which)

  if (field === 'vault_id') {
    return sections.byId(version.vault_id)?.name ?? '—'
  }

  if (isSecretField(field)) {
    const opened = secrets.shown[field]
    if (opened !== null) return opened[which] ?? 'пусто'
    return hasSecret(version, field) ? MASK : 'пусто'
  }

  return conflictMetaValue(version, field)
}

function headerNote(which: ConflictSide): string {
  const version = versionOf(which)
  const when = formatDateTime(version.updated_at)
  const order = later.value === which ? 'позже' : later.value === null ? '' : 'раньше'
  return order === '' ? `${when} · версия № ${version.version}` : `${when} · ${order}`
}

const chosenLabel = computed(() =>
  side.value === null
    ? 'Выберите версию'
    : `Оставить версию · ${versionOf(side.value).device_name}`,
)

async function resolve(): Promise<void> {
  if (side.value === null) return

  saving.value = true
  error.value = null
  try {
    await conflicts.resolve(props.conflict.record_id, side.value)
    toast.push(`Оставлена версия «${versionOf(side.value).device_name}»`, 'success')
    emit('close')
  } catch (cause) {
    error.value = isCoreError(cause) ? cause.message : 'Не удалось сохранить выбор.'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <SyModal open size="wide" title="Две версии одной записи" @close="emit('close')">
    <p class="conflict__lead">
      Запись правили на двух устройствах, пока они не виделись. Ничего не потеряно: обе версии целы.
      Выберите одну целиком — Syncra не выбирает за вас и не склеивает версии по полям.
    </p>

    <div class="conflict__grid" role="radiogroup" aria-label="Какую версию оставить">
      <div class="conflict__corner" aria-hidden="true" />

      <label
        v-for="which in ['local', 'remote'] as ConflictSide[]"
        :key="which"
        class="conflict__head"
        :class="{ 'conflict__head--on': side === which }"
      >
        <input
          v-model="side"
          class="conflict__radio"
          type="radio"
          name="conflict-side"
          :value="which"
        />
        <span class="conflict__head-text">
          <span class="conflict__device">{{ versionOf(which).device_name }}</span>
          <span class="conflict__when">{{ headerNote(which) }}</span>
        </span>
      </label>

      <template v-for="field in rows" :key="field">
        <div class="conflict__label">
          <span>{{ conflictFieldLabel(field) }}</span>
          <button
            v-if="isSecretField(field) && differs(field)"
            type="button"
            class="conflict__reveal"
            :disabled="secrets.busy.value === field"
            @click="secrets.toggle(field)"
          >
            {{ secrets.shown[field] === null ? 'Показать' : `Скрыть · ${secrets.hideIn[field]} с` }}
          </button>
        </div>

        <div
          v-for="which in ['local', 'remote'] as ConflictSide[]"
          :key="`${field}:${which}`"
          class="conflict__cell"
          :class="{
            'conflict__cell--differs': differs(field),
            'conflict__cell--on': side === which,
            'conflict__cell--secret': isSecretField(field),
          }"
        >
          {{ cellValue(which, field) }}
        </div>
      </template>
    </div>

    <p v-if="secrets.error.value" class="conflict__error" role="alert">
      {{ secrets.error.value }}
    </p>
    <p v-if="error" class="conflict__error" role="alert">{{ error }}</p>

    <template #actions>
      <span class="conflict__diff-line">{{ differingLine(conflict) }}</span>
      <SyButton size="sm" :disabled="saving" @click="emit('close')">Решить позже</SyButton>
      <SyButton
        variant="primary"
        size="sm"
        :disabled="side === null"
        :loading="saving"
        @click="resolve"
      >
        {{ chosenLabel }}
      </SyButton>
    </template>
  </SyModal>
</template>

<style scoped>
.conflict__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.conflict__grid {
  display: grid;
  grid-template-columns: 104px minmax(0, 1fr) minmax(0, 1fr);
  gap: var(--sy-space-1) var(--sy-space-3);
  align-items: stretch;
  margin-top: var(--sy-space-5);
}

.conflict__corner {
  grid-column: 1;
}

.conflict__head {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  padding: var(--sy-space-4) var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-bg-1);
  cursor: pointer;
}

.conflict__head--on {
  border-color: var(--sy-accent);
  background: var(--sy-accent-quiet);
}

.conflict__head:focus-within {
  box-shadow: var(--sy-focus-ring);
}

.conflict__radio {
  flex: none;
  width: 15px;
  height: 15px;
  margin: 0;
  accent-color: var(--sy-accent);
}

.conflict__head-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.conflict__device {
  font-size: 14.5px;
  font-weight: var(--sy-weight-semibold);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conflict__when {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.conflict__label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  justify-content: center;
  padding: var(--sy-space-2) 0;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.conflict__reveal {
  align-self: flex-start;
  padding: 0;
  border: none;
  background: none;
  color: var(--sy-accent);
  font-family: var(--sy-font-mono);
  font-size: 9.5px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  cursor: pointer;
}

.conflict__reveal:disabled {
  color: var(--sy-text-3);
  cursor: default;
}

.conflict__cell {
  display: flex;
  align-items: center;
  min-height: 34px;
  padding: var(--sy-space-2) var(--sy-space-5);
  border: 1px solid transparent;
  border-radius: var(--sy-radius-xs);
  font-size: var(--sy-text-body);
  color: var(--sy-text-3);
  overflow-wrap: anywhere;
}

/* Совпавшее поле — приглушено: смотреть надо на то, что разошлось. */
.conflict__cell--differs {
  border-color: var(--sy-border-strong);
  background: var(--sy-surface);
  color: var(--sy-text);
}

.conflict__cell--secret {
  font-family: var(--sy-font-mono);
  font-size: 12.5px;
}

.conflict__cell--on.conflict__cell--differs {
  border-color: var(--sy-accent-border);
}

.conflict__diff-line {
  flex: 1;
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.conflict__error {
  margin-top: var(--sy-space-4);
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>
