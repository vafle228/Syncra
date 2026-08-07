<script setup lang="ts">
import { computed } from 'vue'

import { SyButton } from '@/components/ui'
import type { GeneratorMode, GeneratorProfile, WordSeparator } from '@/core/contract'
import { GENERATOR_LIMITS } from '@/core/contract'

/**
 * Редактор профиля генерации (F6, §3.11 макета).
 *
 * Компонент управляемый: правила приходят сверху и уходят обратно целиком.
 * Своей копии профиля он не держит — иначе экран настроек и панель в форме
 * записи разъехались бы, показывая разные правила одному пользователю.
 *
 * Секретов здесь нет вообще: профиль — это настройки. Ни один сгенерированный
 * пароль через этот компонент не проходит.
 *
 * Подписи взяты из макета дословно: каждая объясняет ЦЕНУ выбора, а не просто
 * называет тумблер («Часть старых сайтов их не принимает — тогда выключите»).
 */

const props = defineProps<{
  modelValue: GeneratorProfile
  /** Отличаются ли правила от сохранённых в ядре. */
  dirty: boolean
  saving?: boolean
  /** Сообщение об ошибке сохранения. */
  error?: string | null
}>()

const emit = defineEmits<{
  'update:modelValue': [profile: GeneratorProfile]
  save: []
}>()

/** Любая правка отдаёт наверх ЦЕЛЫЙ профиль: частичных состояний не бывает. */
function patch(changes: Partial<GeneratorProfile>): void {
  emit('update:modelValue', { ...props.modelValue, ...changes })
}

const isWords = computed(() => props.modelValue.mode === 'words')

const MODES: { value: GeneratorMode; label: string }[] = [
  { value: 'words', label: 'слова' },
  { value: 'chars', label: 'символы' },
]

const SEPARATORS: { value: WordSeparator; label: string }[] = [
  { value: '-', label: '-' },
  { value: '.', label: '.' },
  { value: ' ', label: 'пробел' },
]

interface Toggle {
  key: keyof GeneratorProfile
  title: string
  body: string
  value: boolean
}

/** Тумблеры зависят от режима: у фразы и у набора символов разные ручки. */
const toggles = computed<Toggle[]>(() =>
  isWords.value
    ? [
        {
          key: 'append_number',
          title: 'Добавлять число в конец',
          body: 'Некоторые сайты требуют цифру — пусть будет сразу.',
          value: props.modelValue.append_number,
        },
      ]
    : [
        {
          key: 'digits',
          title: 'Цифры',
          body: '2–9, единицу и ноль пропускаем.',
          value: props.modelValue.digits,
        },
        {
          key: 'symbols',
          title: 'Символы',
          body: 'Часть старых сайтов их не принимает — тогда выключите.',
          value: props.modelValue.symbols,
        },
        {
          key: 'avoid_ambiguous',
          title: 'Без похожих символов',
          body: 'Ни l/I, ни O/0 — чтобы можно было продиктовать вслух.',
          value: props.modelValue.avoid_ambiguous,
        },
      ],
)

function toggle(item: Toggle): void {
  patch({ [item.key]: !item.value } as Partial<GeneratorProfile>)
}

function setNumber(field: 'length' | 'words', event: Event): void {
  patch({ [field]: Number((event.target as HTMLInputElement).value) })
}
</script>

<template>
  <div class="gp">
    <div class="gp__modes" role="group" aria-label="Из чего собирать пароль">
      <button
        v-for="mode in MODES"
        :key="mode.value"
        type="button"
        class="gp__chip"
        :class="{ 'gp__chip--on': modelValue.mode === mode.value }"
        :aria-pressed="modelValue.mode === mode.value"
        @click="patch({ mode: mode.value })"
      >
        {{ mode.label }}
      </button>
    </div>

    <div v-if="isWords" class="gp__block">
      <div class="gp__slider">
        <div class="gp__slider-head">
          <label class="gp__slider-label" for="gp-words">Сколько слов</label>
          <span class="gp__slider-value">{{ modelValue.words }}</span>
        </div>
        <input
          id="gp-words"
          type="range"
          class="gp__range"
          :min="GENERATOR_LIMITS.words.min"
          :max="GENERATOR_LIMITS.words.max"
          step="1"
          :value="modelValue.words"
          @input="setNumber('words', $event)"
        />
        <p class="gp__hint">
          Четыре русских слова уже надёжнее, чем «Qw!7z» — и их можно продиктовать по телефону.
        </p>
      </div>

      <div class="gp__field">
        <span class="gp__slider-label">Разделитель</span>
        <div class="gp__modes" role="group" aria-label="Разделитель слов">
          <button
            v-for="separator in SEPARATORS"
            :key="separator.value"
            type="button"
            class="gp__chip gp__chip--mono"
            :class="{ 'gp__chip--on': modelValue.separator === separator.value }"
            :aria-pressed="modelValue.separator === separator.value"
            @click="patch({ separator: separator.value })"
          >
            {{ separator.label }}
          </button>
        </div>
      </div>
    </div>

    <div v-else class="gp__slider">
      <div class="gp__slider-head">
        <label class="gp__slider-label" for="gp-length">Длина</label>
        <span class="gp__slider-value">{{ modelValue.length }}</span>
      </div>
      <input
        id="gp-length"
        type="range"
        class="gp__range"
        :min="GENERATOR_LIMITS.length.min"
        :max="GENERATOR_LIMITS.length.max"
        step="1"
        :value="modelValue.length"
        @input="setNumber('length', $event)"
      />
      <p class="gp__hint">
        Короче 12 символов уже неуютно; длиннее 24 всё равно никто не набирает руками.
      </p>
    </div>

    <div class="gp__toggles">
      <button
        v-for="item in toggles"
        :key="item.key"
        type="button"
        class="gp__toggle"
        role="switch"
        :aria-checked="item.value"
        @click="toggle(item)"
      >
        <span class="gp__toggle-text">
          <span class="gp__toggle-title">{{ item.title }}</span>
          <span class="gp__toggle-body">{{ item.body }}</span>
        </span>
        <span class="gp__switch" :class="{ 'gp__switch--on': item.value }" aria-hidden="true">
          <span class="gp__knob" />
        </span>
      </button>
    </div>

    <p v-if="error" class="gp__error" role="alert">{{ error }}</p>

    <div class="gp__foot">
      <p class="gp__foot-note">
        {{
          dirty
            ? 'Правила изменены — сохраните, чтобы они применялись к новым паролям.'
            : 'Правила сохранены и применяются ко всем новым паролям.'
        }}
      </p>
      <SyButton size="sm" :disabled="!dirty" :loading="saving" @click="emit('save')">
        Сохранить как профиль
      </SyButton>
    </div>
  </div>
</template>

<style scoped>
.gp {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
}

.gp__modes {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sy-space-3);
}

.gp__chip {
  height: var(--sy-control-height);
  padding: 0 var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: transparent;
  color: var(--sy-text-2);
  font-family: var(--sy-font-sans);
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
  cursor: pointer;
  transition:
    background var(--sy-transition),
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.gp__chip:hover {
  border-color: var(--sy-border-strong);
  color: var(--sy-text);
}

.gp__chip--mono {
  font-family: var(--sy-font-mono);
}

.gp__chip--on {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-text);
}

.gp__block {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
}

.gp__field,
.gp__slider {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  min-width: 0;
}

.gp__slider-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-5);
}

.gp__slider-label {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
}

.gp__slider-value {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-body);
  color: var(--sy-accent);
}

.gp__range {
  width: 100%;
  accent-color: var(--sy-accent);
  cursor: pointer;
}

.gp__hint {
  font-size: var(--sy-text-small);
  line-height: var(--sy-text-small-lh);
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.gp__toggles {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
}

.gp__toggle {
  display: flex;
  align-items: center;
  gap: var(--sy-space-7);
  width: 100%;
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: border-color var(--sy-transition);
}

.gp__toggle:hover {
  border-color: var(--sy-border-strong);
}

.gp__toggle:focus-visible {
  outline: none;
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.gp__toggle-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.gp__toggle-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.gp__toggle-body {
  font-size: var(--sy-text-small);
  line-height: var(--sy-text-small-lh);
  color: var(--sy-text-2);
}

.gp__switch {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  width: 38px;
  height: 22px;
  padding: 2px;
  border-radius: var(--sy-radius-pill);
  background: var(--sy-surface-2);
  transition:
    background var(--sy-transition),
    justify-content var(--sy-transition);
}

.gp__switch--on {
  justify-content: flex-end;
  background: var(--sy-accent);
}

.gp__knob {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--sy-text-3);
}

.gp__switch--on .gp__knob {
  background: var(--sy-accent-fg);
}

.gp__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding-top: var(--sy-space-5);
  border-top: 1px solid var(--sy-border);
}

.gp__foot-note {
  flex: 1;
  min-width: 0;
  font-size: var(--sy-text-small);
  line-height: 1.5;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.gp__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>
