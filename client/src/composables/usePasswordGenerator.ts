import { computed, onScopeDispose, ref } from 'vue'

import type { GeneratorProfile } from '@/core/contract'
import { GENERATOR_DEFAULT_COUNT } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'
import { useGeneratorStore } from '@/stores/useGeneratorStore'

/**
 * Варианты пароля от ядра (F6, §6.1).
 *
 * ЗАКОН №1. Сгенерированный пароль — ещё ничей секрет: он не лежит в хранилище
 * и исчезнет бесследно, если пользователь не выберет ни одного варианта. Но
 * обращаемся с ним как с секретом:
 *  - варианты живут в локальном `ref` этого composable — то есть в области
 *    видимости компонента, а не в Pinia и не в localStorage;
 *  - они исчезают при закрытии панели, при размонтировании и при блокировке
 *    хранилища;
 *  - генерация целиком в ядре: сюда приходят готовые строки, никакой
 *    случайности и никаких алфавитов на фронте нет.
 *
 * Авто-скрытия по таймеру здесь СОЗНАТЕЛЬНО нет, в отличие от `useRecordSecrets`.
 * Там таймер защищает чужой сохранённый пароль, оставленный на экране; здесь
 * пользователь стоит перед списком и выбирает — список, исчезающий из-под руки
 * посреди выбора, мешал бы, ничего не защищая: эти строки ещё нигде не значат
 * доступа.
 */

export function usePasswordGenerator() {
  /** Варианты на выбор. Пустой массив — панель ничего не показывает. */
  const variants = ref<string[]>([])
  /** Оценка стойкости от ЯДРА. Фронт её не пересчитывает. */
  const entropyBits = ref(0)
  /**
   * Какой вариант отмечен. Именно ИНДЕКС, а не значение: номер строки — не
   * секрет, и хранить рядом вторую копию пароля незачем.
   */
  const pickedIndex = ref<number | null>(null)
  const busy = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит. */
  const error = ref<string | null>(null)

  const hasVariants = computed(() => variants.value.length > 0)

  /** Забыть варианты: панель закрыли, запись сохранили, хранилище заперли. */
  function forget(): void {
    variants.value = []
    entropyBits.value = 0
    pickedIndex.value = null
  }

  /**
   * Попросить у ядра `count` вариантов.
   *
   * `profile` — разовые правила для предпросмотра ещё не сохранённых настроек.
   * Не указан — ядро возьмёт сохранённый профиль.
   */
  async function generate(
    count: number = GENERATOR_DEFAULT_COUNT,
    profile?: GeneratorProfile,
  ): Promise<boolean> {
    busy.value = true
    error.value = null
    try {
      const result = await useCore().generatePasswords(count, profile)
      variants.value = result.passwords
      entropyBits.value = result.entropy_bits
      // Список сменился — прежняя отметка указывала бы на чужую строку.
      pickedIndex.value = null
      return true
    } catch (cause) {
      forget()
      error.value = isCoreError(cause) ? cause.message : 'Ядро не смогло сгенерировать пароль.'
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * Отметить вариант и отдать его наружу. Значение возвращается вызывающему
   * (форма кладёт его в поле пароля) и здесь дополнительно не сохраняется.
   */
  function pick(index: number): string | null {
    const value = variants.value[index]
    if (value === undefined) return null

    pickedIndex.value = index
    return value
  }

  // Хранилище заперли — вариантов на экране остаться не должно.
  let unsubscribe: Unsubscribe | null = null
  try {
    unsubscribe = useCore().on('locked', forget)
  } catch {
    /* ядро ещё не поднято — подписка не критична для рендера */
  }

  onScopeDispose(() => {
    forget()
    unsubscribe?.()
    unsubscribe = null
  })

  return { variants, entropyBits, pickedIndex, busy, error, hasVariants, generate, pick, forget }
}

// ---------------------------------------------------------------------------
// Черновик профиля
// ---------------------------------------------------------------------------

const PROFILE_KEYS = [
  'mode',
  'length',
  'digits',
  'symbols',
  'avoid_ambiguous',
  'words',
  'separator',
  'append_number',
] as const satisfies readonly (keyof GeneratorProfile)[]

export function sameProfile(a: GeneratorProfile, b: GeneratorProfile): boolean {
  return PROFILE_KEYS.every((key) => a[key] === b[key])
}

/**
 * Правки профиля до сохранения (F6).
 *
 * Правила редактируются локально, а в ядро уходят одним явным «Сохранить»:
 * §6.1 обещает профиль, настроенный ОДИН РАЗ, — незаметно переписывать его при
 * каждом движении ползунка было бы прямо противоположным поведением. Пока
 * черновик не сохранён, предпросмотр всё равно считается по нему: пользователь
 * должен видеть последствия правки до того, как согласится на неё.
 */
export function useGeneratorProfileDraft() {
  const store = useGeneratorStore()

  const draft = ref<GeneratorProfile | null>(null)
  /** Ошибка именно сохранения — показывается рядом с кнопкой, а не вместо настроек. */
  const saveError = ref<string | null>(null)

  const dirty = computed(
    () =>
      draft.value !== null && store.profile !== null && !sameProfile(draft.value, store.profile),
  )

  /** Загрузить профиль у ядра (один раз на сеанс) и снять с него черновик. */
  async function ensure(): Promise<GeneratorProfile | null> {
    const stored = await store.ensure()
    if (stored !== null && draft.value === null) draft.value = { ...stored }
    return draft.value
  }

  function set(next: GeneratorProfile): void {
    draft.value = next
  }

  async function save(): Promise<boolean> {
    if (draft.value === null) return false

    saveError.value = null
    try {
      draft.value = { ...(await store.save(draft.value)) }
      return true
    } catch (cause) {
      saveError.value = isCoreError(cause) ? cause.message : 'Не удалось сохранить профиль.'
      return false
    }
  }

  return {
    draft,
    dirty,
    saveError,
    saving: computed(() => store.saving),
    loading: computed(() => store.loading),
    loadError: computed(() => store.error),
    ensure,
    set,
    save,
  }
}

// ---------------------------------------------------------------------------
// Предпросмотр
// ---------------------------------------------------------------------------

/**
 * Пауза перед пересчётом предпросмотра. Ползунок длины шлёт событие на каждый
 * шаг — без паузы одно движение мышью означало бы два десятка команд ядру.
 */
export const PREVIEW_DEBOUNCE_MS = 250

/** Отложенный вызов, который сам снимается вместе с компонентом. */
export function useDebounced(fn: () => void, delayMs: number = PREVIEW_DEBOUNCE_MS) {
  let timer: ReturnType<typeof setTimeout> | null = null

  function cancel(): void {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  onScopeDispose(cancel)

  return () => {
    cancel()
    timer = setTimeout(() => {
      timer = null
      fn()
    }, delayMs)
  }
}
