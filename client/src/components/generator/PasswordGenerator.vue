<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'

import { SyButton } from '@/components/ui'
import {
  useDebounced,
  useGeneratorProfileDraft,
  usePasswordGenerator,
} from '@/composables/usePasswordGenerator'
import type { GeneratorProfile } from '@/core/contract'
import { GENERATOR_DEFAULT_COUNT } from '@/core/contract'

import GeneratorProfileForm from './GeneratorProfileForm.vue'

/**
 * Панель генератора в форме записи (F6, §3.4 макета).
 *
 * ЗАКОН №1: варианты живут в `usePasswordGenerator` — в области видимости
 * этого компонента. В Pinia уходит только профиль (настройки), сами пароли —
 * никогда. Панель закрыли или хранилище заперли — вариантов больше нет.
 *
 * Выбор делает ЧЕЛОВЕК: ни один вариант не подставляется в поле пароля сам.
 * «n вариантов на выбор» — требование §6.1, а не украшение: генератор,
 * молча вставляющий одну строку, отучает смотреть, что именно сохраняется.
 */

const emit = defineEmits<{ pick: [password: string]; close: [] }>()

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

function setProfile(next: GeneratorProfile): void {
  profile.set(next)
}
</script>

<template>
  <section class="pg">
    <header class="pg__head">
      <div class="pg__title-row">
        <h3 class="pg__title">Генератор</h3>
        <span class="pg__caption">вариант выбираете вы</span>
      </div>

      <div class="pg__actions">
        <SyButton size="sm" :loading="generator.busy.value" @click="regenerate">
          Ещё варианты
        </SyButton>
        <SyButton size="sm" :aria-expanded="profileOpen" @click="profileOpen = !profileOpen">
          {{ profileOpen ? 'Скрыть правила' : 'Правила' }}
        </SyButton>
        <SyButton size="sm" variant="ghost" @click="emit('close')">Закрыть</SyButton>
      </div>
    </header>

    <div class="pg__body">
      <p v-if="profile.loadError.value" class="pg__error" role="alert">
        {{ profile.loadError.value }}
      </p>
      <p v-if="generator.error.value" class="pg__error" role="alert">
        {{ generator.error.value }}
      </p>

      <template v-if="generator.hasVariants.value">
        <span class="pg__label">Выберите один</span>

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
          <span class="pg__entropy">≈ {{ generator.entropyBits.value }} бит энтропии</span>
          <span class="pg__source">случайность берётся у ОС</span>
        </div>
      </template>

      <p v-else-if="!generator.busy.value && generator.error.value === null" class="pg__empty">
        Вариантов пока нет — нажмите «Ещё варианты».
      </p>
    </div>

    <div v-if="profileOpen" class="pg__profile">
      <div class="pg__profile-head">
        <span class="pg__label">Профиль генерации · настраивается один раз</span>
        <span class="pg__source">Применяется ко всем новым паролям · §3.11</span>
      </div>

      <GeneratorProfileForm
        v-if="profile.draft.value"
        :model-value="profile.draft.value"
        :dirty="profile.dirty.value"
        :saving="profile.saving.value"
        :error="profile.saveError.value"
        @update:model-value="setProfile"
        @save="profile.save()"
      />
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

.pg__body {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  padding: var(--sy-space-6);
}

.pg__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
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
  height: var(--sy-control-height-sm);
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
  font-size: var(--sy-text-small);
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

.pg__profile {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
  padding: var(--sy-space-6);
  border-top: 1px solid var(--sy-border);
  background: var(--sy-bg-1);
}

.pg__profile-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
}
</style>
