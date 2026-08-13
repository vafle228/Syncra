<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'

import { SyButton } from '@/components/ui'
import { pluralize } from '@/composables/plural'
import {
  useDebounced,
  useGeneratorProfileDraft,
  usePasswordGenerator,
} from '@/composables/usePasswordGenerator'
import type { GeneratorProfile, WordSeparator } from '@/core/contract'
import { GENERATOR_DEFAULT_COUNT, GENERATOR_LIMITS } from '@/core/contract'

/**
 * Блок «Пароль» в форме записи (F6, §3.4 макета).
 *
 * Из макета: это не отдельная «панель генератора», которую открывают кнопкой, а
 * сам блок пароля — с вариантами на виду. Ниже вариантов строка состояния:
 * «текущий пароль остаётся, пока не выбран новый», — поэтому открытый список
 * ничего не заменяет молча, и прятать его незачем.
 *
 * ЗАКОН №1: варианты живут в `usePasswordGenerator` — в области видимости
 * этого компонента. В Pinia уходит только профиль (настройки), сами пароли —
 * никогда. Компонент размонтировали или хранилище заперли — вариантов больше нет.
 *
 * Выбор делает ЧЕЛОВЕК: ни один вариант не подставляется в поле пароля сам.
 * «n вариантов на выбор» — требование §6.1, а не украшение: генератор,
 * молча вставляющий одну строку, отучает смотреть, что именно сохраняется.
 *
 * Профиль здесь правится в СВЁРНУТОМ виде: ползунок и наборы символов — то, что
 * меняют на ходу. Полный редактор правил (режим фразы, разделитель, похожие
 * символы) живёт в настройках — §6.1 обещает профиль, настроенный один раз.
 */

defineProps<{
  /** Левая половина строки состояния: её знает форма, а не панель. */
  note: string
  /** Правая половина той же строки: «без изменений», «пароль ещё не выбран». */
  noteValue: string
}>()

const emit = defineEmits<{ pick: [password: string] }>()

const generator = usePasswordGenerator()
const profile = useGeneratorProfileDraft()

const profileOpen = ref(false)

/** Перегенерировать по текущему черновику правил (он же сохранённый, пока не правили). */
function regenerate(): void {
  void generator.generate(GENERATOR_DEFAULT_COUNT, profile.draft.value ?? undefined)
}

const regenerateSoon = useDebounced(regenerate)

onMounted(async () => {
  // Сначала правила, потом варианты: генерировать по чужому профилю и тут же
  // переделывать — мигание на ровном месте.
  await profile.ensure()
  regenerate()
})

// Правку правил показываем сразу на вариантах: иначе непонятно, что даёт
// ползунок. Ядро при этом получает разовые правила — сохранённый профиль
// не меняется, пока не нажали «Сохранить как профиль».
watch(
  () => profile.draft.value,
  (next, previous) => {
    if (previous !== null && next !== null) regenerateSoon()
  },
  { deep: true },
)

function choose(index: number): void {
  const password = generator.pick(index)
  if (password !== null) emit('pick', password)
}

// ---------------------------------------------------------------------------
// Свёрнутый профиль: ползунок + наборы символов
// ---------------------------------------------------------------------------

/** Правка правил отдаётся черновику ЦЕЛИКОМ: частичных профилей не бывает. */
function patch(changes: Partial<GeneratorProfile>): void {
  const draft = profile.draft.value
  if (draft !== null) profile.set({ ...draft, ...changes })
}

interface Slider {
  label: string
  value: number
  min: number
  max: number
  apply: (value: number) => void
}

/** У фразы и у набора символов ползунок разный — но место у него одно. */
const slider = computed<Slider | null>(() => {
  const draft = profile.draft.value
  if (draft === null) return null

  return draft.mode === 'words'
    ? {
        label: 'Сколько слов',
        value: draft.words,
        ...GENERATOR_LIMITS.words,
        apply: (value: number) => patch({ words: value }),
      }
    : {
        label: 'Длина',
        value: draft.length,
        ...GENERATOR_LIMITS.length,
        apply: (value: number) => patch({ length: value }),
      }
})

interface Chip {
  name: string
  on: boolean
  /** Выключить нельзя: алфавита без букв не бывает (см. контракт). */
  fixed: boolean
  toggle: () => void
}

const SEPARATORS: { value: WordSeparator; label: string }[] = [
  { value: '-', label: '-' },
  { value: '.', label: '.' },
  { value: ' ', label: 'пробел' },
]

const chipsLabel = computed(() =>
  profile.draft.value?.mode === 'words' ? 'Разделитель' : 'Наборы символов',
)

const chips = computed<Chip[]>(() => {
  const draft = profile.draft.value
  if (draft === null) return []

  if (draft.mode === 'words') {
    return SEPARATORS.map((separator) => ({
      name: separator.label,
      on: draft.separator === separator.value,
      fixed: false,
      toggle: () => patch({ separator: separator.value }),
    }))
  }

  return [
    { name: 'a–z', on: true, fixed: true, toggle: () => {} },
    { name: 'A–Z', on: true, fixed: true, toggle: () => {} },
    { name: '2–9', on: draft.digits, fixed: false, toggle: () => patch({ digits: !draft.digits }) },
    {
      name: '!@#$',
      on: draft.symbols,
      fixed: false,
      toggle: () => patch({ symbols: !draft.symbols }),
    },
  ]
})

/**
 * Подпись под правилами. Про похожие символы говорим ровно то, что сейчас
 * правда: отдельного тумблера здесь нет, но врать, что они «исключены всегда»,
 * нельзя — в настройках его могли выключить.
 */
const profileNote = computed(() => {
  const draft = profile.draft.value
  if (draft !== null && draft.mode === 'chars' && !draft.avoid_ambiguous) {
    return 'Похожие символы (0/O, 1/l) сейчас разрешены — переключается в настройках генератора. Профиль применяется ко всем новым паролям.'
  }
  return 'Похожие символы (0/O, 1/l) исключены — пароль остаётся читаемым, когда его диктуют вслух. Профиль применяется ко всем новым паролям.'
})

/** Строка стойкости из макета: сначала правила, потом их цена в битах. */
const entropyText = computed(() => {
  const draft = profile.draft.value
  const bits = generator.entropyBits.value
  if (draft === null) return `≈ ${bits} бит энтропии`

  const amount =
    draft.mode === 'words'
      ? pluralize(draft.words, ['слово', 'слова', 'слов'])
      : pluralize(draft.length, ['символ', 'символа', 'символов'])
  return `Профиль: ${amount} · ≈ ${bits} бит энтропии`
})

function onSlider(event: Event): void {
  slider.value?.apply(Number((event.target as HTMLInputElement).value))
}
</script>

<template>
  <section class="pg">
    <header class="pg__head">
      <div class="pg__title-row">
        <h3 class="pg__title">Пароль</h3>
        <span class="pg__caption">генератор · вариант выбираете вы</span>
      </div>

      <div class="pg__actions">
        <SyButton size="sm" :loading="generator.busy.value" @click="regenerate">
          Ещё варианты
        </SyButton>
        <SyButton
          class="pg__profile-toggle"
          :class="{ 'pg__profile-toggle--on': profileOpen }"
          size="sm"
          :aria-expanded="profileOpen"
          @click="profileOpen = !profileOpen"
        >
          {{ profileOpen ? 'Свернуть профиль' : 'Профиль генерации' }}
        </SyButton>
      </div>
    </header>

    <div v-if="profileOpen" class="pg__profile">
      <p v-if="profile.loadError.value" class="pg__error" role="alert">
        {{ profile.loadError.value }}
      </p>

      <div v-if="slider" class="pg__profile-grid">
        <div class="pg__slider">
          <div class="pg__slider-head">
            <label class="pg__slider-label" for="pg-slider">{{ slider.label }}</label>
            <span class="pg__slider-value">{{ slider.value }}</span>
          </div>
          <input
            id="pg-slider"
            type="range"
            class="pg__range"
            :min="slider.min"
            :max="slider.max"
            step="1"
            :value="slider.value"
            @input="onSlider"
          />
        </div>

        <div class="pg__sets">
          <span class="pg__slider-label">{{ chipsLabel }}</span>
          <div class="pg__chips" role="group" :aria-label="chipsLabel">
            <template v-for="chip in chips" :key="chip.name">
              <!--
                Буквы из алфавита не убираются — и кнопкой, которая ничего не
                делает, притворяться не должны.
              -->
              <span v-if="chip.fixed" class="pg__chip pg__chip--on pg__chip--fixed">
                {{ chip.name }}
              </span>
              <button
                v-else
                type="button"
                class="pg__chip"
                :class="{ 'pg__chip--on': chip.on }"
                :aria-pressed="chip.on"
                @click="chip.toggle"
              >
                {{ chip.name }}
              </button>
            </template>
          </div>
        </div>
      </div>

      <p v-if="profile.saveError.value" class="pg__error" role="alert">
        {{ profile.saveError.value }}
      </p>

      <div class="pg__profile-foot">
        <p class="pg__profile-note">{{ profileNote }}</p>
        <SyButton
          size="sm"
          :disabled="!profile.dirty.value"
          :loading="profile.saving.value"
          @click="profile.save()"
        >
          Сохранить как профиль
        </SyButton>
      </div>
    </div>

    <div class="pg__body">
      <!-- Ручной ввод пароля даёт форма: свой пароль — такой же черновик. -->
      <slot />

      <div class="pg__status">
        <span class="pg__label">{{ note }}</span>
        <span class="pg__status-value">{{ noteValue }}</span>
      </div>

      <p v-if="generator.error.value" class="pg__error" role="alert">
        {{ generator.error.value }}
      </p>

      <template v-if="generator.hasVariants.value">
        <ul class="pg__variants">
          <li v-for="(variant, index) in generator.variants.value" :key="index">
            <button
              type="button"
              class="pg__variant"
              :class="{ 'pg__variant--picked': generator.pickedIndex.value === index }"
              :aria-pressed="generator.pickedIndex.value === index"
              @click="choose(index)"
            >
              <span class="pg__radio" aria-hidden="true" />
              <span class="pg__value">{{ variant }}</span>
              <span v-if="generator.pickedIndex.value === index" class="pg__picked">Выбран</span>
            </button>
          </li>
        </ul>

        <div class="pg__foot">
          <span class="pg__entropy">{{ entropyText }}</span>
          <span class="pg__source">случайность берётся у ОС</span>
        </div>
      </template>

      <p v-else-if="!generator.busy.value && generator.error.value === null" class="pg__empty">
        Вариантов пока нет — нажмите «Ещё варианты».
      </p>
    </div>
  </section>
</template>

<style scoped>
.pg {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
  overflow: hidden;
}

.pg__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
}

.pg__title-row {
  display: flex;
  align-items: baseline;
  gap: var(--sy-space-5);
  min-width: 0;
}

.pg__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.pg__caption {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.pg__actions {
  flex: none;
  display: flex;
  gap: var(--sy-space-3);
}

/* Открытый профиль отмечен на самой кнопке: иначе непонятно, откуда панель. */
.pg__profile-toggle--on {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
  font-weight: var(--sy-weight-semibold);
}

.pg__profile {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-1);
  animation: sy-in 0.16s ease-out;
}

.pg__profile-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: var(--sy-space-5) var(--sy-space-7);
  align-items: end;
}

.pg__slider,
.pg__sets {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  min-width: 0;
}

.pg__slider-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-5);
}

.pg__slider-label {
  font-size: 13px;
  color: var(--sy-text-2);
}

.pg__slider-value {
  font-family: var(--sy-font-mono);
  font-size: 14px;
  color: var(--sy-text);
}

.pg__range {
  width: 100%;
  accent-color: var(--sy-accent);
  cursor: pointer;
}

.pg__chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sy-space-3);
}

.pg__chip {
  display: inline-flex;
  align-items: center;
  height: var(--sy-control-height-sm);
  padding: 0 var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-xs);
  background: transparent;
  color: var(--sy-text-3);
  font-family: var(--sy-font-mono);
  font-size: 12px;
  cursor: pointer;
  transition:
    background var(--sy-transition),
    border-color var(--sy-transition),
    color var(--sy-transition);
}

button.pg__chip:hover {
  border-color: var(--sy-border-strong);
  color: var(--sy-text-2);
}

button.pg__chip:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.pg__chip--on {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.pg__chip--fixed {
  cursor: default;
}

.pg__profile-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding-top: var(--sy-space-4);
  border-top: 1px solid var(--sy-border);
}

.pg__profile-note {
  max-width: 520px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.pg__body {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  padding: var(--sy-space-5) var(--sy-space-6);
}

.pg__status {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-5);
}

.pg__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.pg__status-value {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 12px;
  color: var(--sy-text-2);
}

.pg__variants {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  margin: 0;
  padding: 0;
  list-style: none;
}

.pg__variant {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  width: 100%;
  height: 44px;
  padding: 0 var(--sy-space-3) 0 var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  color: var(--sy-text-2);
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    background var(--sy-transition),
    color var(--sy-transition);
}

.pg__variant:hover {
  border-color: var(--sy-border-strong);
  background: var(--sy-surface-2);
}

.pg__variant:focus-visible {
  outline: none;
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.pg__variant--picked {
  border-color: var(--sy-accent);
  background: var(--sy-accent-quiet);
  color: var(--sy-text);
}

.pg__radio {
  flex: none;
  display: grid;
  place-items: center;
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--sy-text-3);
  border-radius: 50%;
}

.pg__variant--picked .pg__radio {
  border-color: var(--sy-accent);
}

.pg__variant--picked .pg__radio::after {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-accent);
  content: '';
}

.pg__value {
  flex: 1;
  min-width: 0;
  font-family: var(--sy-font-mono);
  font-size: 14.5px;
  letter-spacing: 0.04em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pg__picked {
  flex: none;
  display: grid;
  place-items: center;
  height: 30px;
  padding: 0 var(--sy-space-5);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-xs);
  color: var(--sy-accent);
  font-size: 12px;
}

.pg__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding-top: var(--sy-space-1);
}

.pg__entropy {
  font-size: 12px;
  color: var(--sy-text-2);
}

.pg__source {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.pg__empty {
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
}

.pg__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>
