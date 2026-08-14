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
  <!--
    Полосный диалог макета (`Прототип:2378-2438`): вопрос в шапке, обе версии в
    прокручиваемом теле, ответ в подвале. Шапка и кнопки не уезжают вместе с
    содержимым — выбирать версию, потеряв из виду вопрос, не приходится.
  -->
  <SyModal open banded size="wide" title="Две версии одной записи" @close="emit('close')">
    <template #lead>
      Запись правили офлайн на двух устройствах. Ничего не потеряно: обе версии целы — выберите одну
      целиком. Syncra не выбирает за вас и не склеивает версии по полям.
    </template>

    <div class="conflict__grid" role="radiogroup" aria-label="Какую версию оставить">
      <!--
        Версия — это КАРТОЧКА, а не колонка таблицы: выбирают запись целиком,
        и подсвечиваться должна вся она, а не отдельные ячейки (§5.5).
      -->
      <div
        v-for="which in ['local', 'remote'] as ConflictSide[]"
        :key="which"
        class="conflict__card"
        :class="{ 'conflict__card--on': side === which }"
        @click="side = which"
      >
        <div class="conflict__card-head">
          <input
            :id="`conflict-${which}`"
            v-model="side"
            class="conflict__radio"
            type="radio"
            name="conflict-side"
            :value="which"
          />
          <label class="conflict__device" :for="`conflict-${which}`">
            {{ versionOf(which).device_name }}
          </label>
          <span class="conflict__when">{{ headerNote(which) }}</span>
        </div>

        <div v-for="field in rows" :key="field" class="conflict__row">
          <div class="conflict__label">
            <span>{{ conflictFieldLabel(field) }}</span>
            <button
              v-if="isSecretField(field) && differs(field)"
              type="button"
              class="conflict__reveal"
              :disabled="secrets.busy.value === field"
              @click.stop="secrets.toggle(field)"
            >
              {{
                secrets.shown[field] === null ? 'Показать' : `Скрыть · ${secrets.hideIn[field]} с`
              }}
            </button>
          </div>

          <div
            class="conflict__cell"
            :class="{
              'conflict__cell--differs': differs(field),
              'conflict__cell--on': side === which,
            }"
          >
            {{ cellValue(which, field) }}
          </div>
        </div>
      </div>
    </div>

    <p v-if="secrets.error.value" class="conflict__error" role="alert">
      {{ secrets.error.value }}
    </p>
    <p v-if="error" class="conflict__error" role="alert">{{ error }}</p>

    <template #note>
      <span class="conflict__diff-line">{{ differingLine(conflict) }}</span>
    </template>

    <template #actions>
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
.conflict__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--sy-space-6);
  align-items: start;
}

.conflict__card {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-1);
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    background var(--sy-transition);
}

.conflict__card:hover {
  border-color: var(--sy-border-strong);
}

.conflict__card--on {
  border-color: var(--sy-accent);
  background: var(--sy-accent-quiet);
  box-shadow: 0 0 0 3px var(--sy-accent-quiet);
}

.conflict__card:focus-within {
  box-shadow: var(--sy-focus-ring);
}

.conflict__card-head {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  min-width: 0;
}

.conflict__radio {
  flex: none;
  width: 15px;
  height: 15px;
  margin: 0;
  accent-color: var(--sy-accent);
}

.conflict__device {
  flex: 1;
  min-width: 0;
  font-size: var(--sy-text-item-title);
  font-weight: var(--sy-weight-semibold);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.conflict__when {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-meta);
  color: var(--sy-text-3);
}

.conflict__card--on .conflict__when {
  color: var(--sy-accent);
}

.conflict__row {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  min-width: 0;
}

.conflict__label {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-4);
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-tag);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.conflict__reveal {
  flex: none;
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

/*
 * Все значения — моноширинные: их сравнивают посимвольно, а не читают.
 * Совпавшее поле остаётся БЕЗ рамки и приглушено (`Прототип:2400`): рамка
 * здесь означает «вот тут и разошлось», и раздавать её всем подряд — значит
 * заставить искать различие глазами.
 */
.conflict__cell {
  display: flex;
  align-items: center;
  min-height: 32px;
  padding: var(--sy-space-3) 11px;
  border: 1px solid transparent;
  border-radius: var(--sy-radius-inner);
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
  overflow-wrap: anywhere;
}

.conflict__cell--differs {
  border-color: var(--sy-border-strong);
  background: var(--sy-surface);
  color: var(--sy-text-2);
}

.conflict__cell--on.conflict__cell--differs {
  border-color: var(--sy-accent-border);
  color: var(--sy-text);
}

.conflict__diff-line {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-meta);
  color: var(--sy-text-3);
}

.conflict__error {
  margin-top: var(--sy-space-4);
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>
