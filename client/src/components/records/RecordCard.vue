<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

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
import { DEVICE_AT_FORMS, pluralize } from '@/composables/plural'
import { securityPolicy } from '@/composables/securityPolicy'
import { useRecordSecrets } from '@/composables/useRecordSecrets'
import { useTotpCode } from '@/composables/useTotpCode'
import type { RecordMeta } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useDevicesStore } from '@/stores/useDevicesStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useSyncStore } from '@/stores/useSyncStore'
import { useToastStore } from '@/stores/useToastStore'

import { formatDate, hostOf, passwordAgeWarning } from './recordFormat'
import RecordTotp from './RecordTotp.vue'

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
const devices = useDevicesStore()
const toast = useToastStore()

onMounted(() => {
  void sections.ensure()
  void conflicts.ensure()
  // Подвал говорит, уехала ли запись; дальше состояние приезжает событиями.
  void sync.ensure()
  // …и на скольких устройствах она лежит — это тоже строка подвала.
  void devices.ensure()
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
/** Код подтверждения живёт отдельно от секретов: у него своё окно жизни. */
const totp = useTotpCode(recordId)

const hue = computed(() => iconHue(props.record.urls[0] ?? props.record.service_name))
const initials = computed(() => iconInitials(props.record.service_name))
const ageWarning = computed(() => passwordAgeWarning(props.record.password_updated_at))

/**
 * Плашка состояния рядом с логином (прототип, шапка карточки). Отвечает на
 * вопрос «а этот пароль вообще есть где-то ещё», не заставляя идти в настройки.
 *
 * Порядок проверок — это приоритет, а не удобство чтения: конфликт заслонять
 * нечем, локальная секция важнее очереди отправки (запись не уедет НИКОГДА, а
 * не «пока»), и лишь потом обычное ожидание обмена.
 */
const syncChip = computed<{ tone: 'accent' | 'warn' | 'danger'; text: string }>(() => {
  if (conflict.value !== null) return { tone: 'danger', text: 'конфликт версий' }
  if (section.value !== null && !section.value.sync) {
    return { tone: 'warn', text: 'только это устройство' }
  }
  if (isPending.value) return { tone: 'warn', text: 'ждёт синхронизации' }
  return { tone: 'accent', text: 'синхронизировано' }
})

// ---------------------------------------------------------------------------
// Адреса
//
// Показываем ХОСТ, копируем ПОЛНЫЙ адрес: имя сайта — это то, что человек
// узнаёт с одного взгляда, а путь и протокол нужны тому, кто адрес заберёт.
// ---------------------------------------------------------------------------

/** Сколько дополнительных адресов показывать, не спрашивая. */
const URL_CHIPS_SHOWN = 2

const extraUrls = computed(() => props.record.urls.slice(1))
const urlsExpanded = ref(false)

const visibleExtras = computed(() =>
  urlsExpanded.value ? extraUrls.value : extraUrls.value.slice(0, URL_CHIPS_SHOWN),
)

const hiddenUrlCount = computed(() => Math.max(0, extraUrls.value.length - URL_CHIPS_SHOWN))

// ---------------------------------------------------------------------------
// Меню «···»
//
// Реплика дизайна: пункт в нём ровно один — признание, что состав редких
// действий ещё не решён. Порт дословный (см. `TASKS.md`), но с настоящей
// семантикой поповера: у прототипа это див с `onClick`.
// ---------------------------------------------------------------------------

const menuOpen = ref(false)
const menuRoot = ref<HTMLElement | null>(null)

function toggleMenu(): void {
  menuOpen.value = !menuOpen.value
}

/**
 * Поповер закрывается щелчком мимо и Escape. Без этого он выглядел бы ловушкой:
 * открыл — и обратно только тем же «···», которого уже не видно за самим меню.
 */
function onDocumentPointerDown(event: MouseEvent): void {
  if (!menuOpen.value) return
  if (menuRoot.value?.contains(event.target as Node)) return
  menuOpen.value = false
}

function onDocumentKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') menuOpen.value = false
}

onMounted(() => {
  document.addEventListener('mousedown', onDocumentPointerDown)
  document.addEventListener('keydown', onDocumentKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onDocumentPointerDown)
  document.removeEventListener('keydown', onDocumentKeydown)
})

/** Копирование метаданных: буфер не чистим — там нет секрета. */
async function copyMeta(key: string, value: string, what: string): Promise<void> {
  if (await clipboard.copy(key, value)) toast.push(`${what} скопирован`, 'success')
  else toast.push('Буфер обмена недоступен', 'danger')
}

/**
 * Копируется из карточки ровно одно секретное поле — пароль. Заметку читают
 * глазами, а код подтверждения меняется быстрее, чем его успевают вставить из
 * буфера: обе кнопки убраны по макету, а не забыты.
 */
async function copyPassword(): Promise<void> {
  const done = await secrets.copy('password')
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
// Подвал
// ---------------------------------------------------------------------------

/**
 * Строка подвала (`Прототип:2896`). Макет не успокаивает («копии есть»), а
 * отвечает на два вопроса: СКОЛЬКО копий и ЧЬЯ версия сейчас показана. Обе
 * оговорки — локальная секция и неуехавшая правка — остаются: без них счёт
 * устройств обещал бы копии, которых нет.
 */
const storedLine = computed(() => {
  if (section.value !== null && !section.value.sync) {
    return `секция «${section.value.name}» локальная · копий этой записи больше нигде нет`
  }
  if (isPending.value) return 'изменена здесь · последняя версия ещё никуда не уехала'

  // Пока ядро не ответило, устройств «ноль» — считать их вслух нельзя.
  const count = devices.active.length
  if (count <= 1) return 'хранится только на этом устройстве · последняя версия отсюда'
  return `хранится на ${pluralize(count, DEVICE_AT_FORMS)} · последняя версия отсюда`
})

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
        <div class="card__login-row">
          <p class="card__login">{{ record.login }}</p>
          <span class="card__sync" :class="`card__sync--${syncChip.tone}`">
            <span class="card__sync-dot" aria-hidden="true" />
            {{ syncChip.text }}
          </span>
        </div>
      </div>

      <div class="card__head-actions">
        <SyButton size="header" data-test="card-edit" @click="emit('edit')">Изменить</SyButton>

        <div ref="menuRoot" class="card__menu">
          <SyButton
            size="header"
            icon
            class="card__menu-button"
            title="Другие действия"
            aria-label="Другие действия"
            :aria-expanded="menuOpen"
            @click="toggleMenu"
          >
            ···
          </SyButton>

          <div v-if="menuOpen" class="card__menu-pop" role="dialog" aria-label="Другие действия">
            <div class="card__menu-head">
              <span class="card__menu-tag">
                <span class="card__menu-dot" aria-hidden="true" />
                Ещё думаем
              </span>
              <p class="card__menu-text">
                Здесь появятся редкие действия — что именно, решаем. В обсуждении: история
                изменений, экспорт одной записи, перенос в другую секцию, доступ для близкого
                человека.
              </p>
            </div>
            <hr class="card__menu-rule" />
            <p class="card__menu-foot">
              Всё, что работает сейчас, лежит на виду: в карточке и в «Изменить».
            </p>
          </div>
        </div>
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

            <template v-else>
              <div class="card__value">
                <span class="card__value-text" :title="record.urls[0]">
                  {{ hostOf(record.urls[0]!) }}
                </span>
                <SyCopyButton
                  compact
                  label="Скопировать адрес"
                  :copied="clipboard.copiedKey.value === 'url:0'"
                  :unavailable="!clipboard.available.value"
                  @click="copyMeta('url:0', record.urls[0]!, 'Адрес')"
                />
              </div>

              <!--
                Остальные адреса — чипами: у одной записи их бывает пять, и пять
                полноразмерных строк вытеснили бы с экрана всё остальное.
                Нажатие копирует полный адрес, как и у первого.
              -->
              <div v-if="visibleExtras.length > 0" class="card__chips">
                <button
                  v-for="(url, index) in visibleExtras"
                  :key="url"
                  type="button"
                  class="card__chip"
                  :title="`Скопировать ${url}`"
                  @click="copyMeta(`url:${index + 1}`, url, 'Адрес')"
                >
                  {{ hostOf(url) }}
                </button>
              </div>

              <button
                v-if="hiddenUrlCount > 0 && !urlsExpanded"
                type="button"
                class="card__more"
                @click="urlsExpanded = true"
              >
                ещё {{ hiddenUrlCount }}
              </button>
              <button
                v-else-if="urlsExpanded && extraUrls.length > URL_CHIPS_SHOWN"
                type="button"
                class="card__more"
                @click="urlsExpanded = false"
              >
                свернуть
              </button>
            </template>
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
              <!--
                Только точка и имя: уедет ли запись, уже сказано плашкой в шапке,
                и повторять это здесь значило бы держать две правды об одном.
              -->
              <span class="card__value-text">{{ section?.name ?? '—' }}</span>
            </div>
          </div>

          <!--
            Номера версии в карточке нет и в макете: счётчик — служебная деталь
            синхронизации, и человеку он говорит меньше, чем даты рядом. Сама
            `record.version` остаётся в сторе — это данные контракта.
          -->
          <div class="card__field">
            <span class="card__field-label">Создано · пароль изменён</span>
            <div class="card__dates">
              <span>{{ formatDate(record.created_at) }}</span>
              <span class="card__dates-sep" aria-hidden="true" />
              <span>{{ formatDate(record.password_updated_at) }}</span>
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
          copy-label="Копировать пароль"
          :value="secrets.shown.password"
          :hide-in="secrets.hideIn.password"
          :busy="secrets.busy.value === 'password'"
          :copied="clipboard.copiedKey.value === 'password'"
          :copy-seconds="clipboard.secondsLeft.value"
          :clipboard-unavailable="!clipboard.available.value"
          @toggle="secrets.toggle('password')"
          @copy="copyPassword()"
        />

        <div v-if="ageWarning" class="card__age" role="status">
          <span class="card__age-dot" aria-hidden="true" />
          <span>{{ ageWarning }}</span>
        </div>

        <p v-if="totp.error.value" class="card__error" role="alert">{{ totp.error.value }}</p>

        <div class="card__grid card__grid--secrets">
          <!--
            Не ключ, а КОД: карточка показывает то, что вводят в чужую форму.
            Сам `totp_secret` она не показывает вовсе — его редактируют в форме,
            а считает код ядро.
          -->
          <RecordTotp
            :present="record.has_totp"
            :code="totp.code.value"
            :seconds-left="totp.secondsLeft.value"
            :period-s="totp.periodS.value"
            :busy="totp.busy.value"
            @toggle="totp.toggle()"
          />

          <!--
            У заметок нет копирования и второй кнопки (`Прототип:1675-1692`):
            заметку читают глазами, а бокс целиком открывает и закрывает её.
          -->
          <SySecretField
            label="Заметки"
            multiline
            skeleton
            :value="secrets.shown.notes"
            :present="record.has_notes"
            empty-text="Заметок нет"
            :hide-in="secrets.hideIn.notes"
            :busy="secrets.busy.value === 'notes'"
            @toggle="secrets.toggle('notes')"
          />
        </div>
      </section>
    </div>

    <footer class="card__foot">
      <span class="card__foot-note">{{ storedLine }}</span>
      <SyButton variant="danger" size="sm" @click="askDelete">Удалить запись</SyButton>
    </footer>

    <!--
      Порядок блоков — из макета (`Прототип:2262-2283`): имя, мета, цена
      решения и только потом оговорка про TOTP. Оговорка идёт ЯНТАРНОЙ внутри
      красной карточки: рамка говорит про потерю записи, полоска — про вход,
      который придётся восстанавливать. Одним цветом эти два риска слиплись бы.
    -->
    <SyModal
      :open="confirming"
      :title="deleteTitle"
      size="confirm"
      tone="danger"
      warning-tone="warning"
      warning-placement="bottom"
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
      <p class="card__confirm-text">
        Запись и её пароль удалятся с этого устройства и со всех остальных при следующей
        синхронизации. Восстановить её из Syncra будет нельзя.
      </p>
      <p v-if="deleteError" class="card__error" role="alert">{{ deleteError }}</p>

      <!-- Сноска в своём слоте: в `#actions` она распирала бы ряд кнопок. -->
      <template #note>Подтвердите повторным нажатием «Удалить запись»</template>

      <template #actions>
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

.card__login-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  min-width: 0;
}

.card__login {
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

/* Плашка «где живёт этот пароль» — рядом с логином, как в прототипе. */
.card__sync {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 24px;
  padding: 0 9px;
  border: 1px solid var(--sy-border);
  border-radius: 12px;
  background: var(--sy-surface);
  font-size: 11.5px;
  color: var(--sy-text-2);
  white-space: nowrap;
}

.card__sync-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-accent);
}

.card__sync--warn .card__sync-dot {
  background: var(--sy-warn);
}

.card__sync--danger {
  border-color: var(--sy-danger);
}

.card__sync--danger .card__sync-dot {
  background: var(--sy-danger);
}

/*
 * `align-items: center` — своё, не унаследованное от шапки: шапка выравнивает
 * по верху ради иконки 46px, а две одинаковые кнопки рядом с ней от этого
 * повисли бы под верхним краем блока имени.
 */
.card__head-actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
}

/* Меню «···» */

.card__menu {
  position: relative;
}

/* Всю геометрию даёт `SyButton size="header" icon`; здесь — только кегль глифа:
   «···» в макете на полразмера мельче подписи соседней кнопки. */
.card__menu-button {
  font-size: 12px;
}

.card__menu-pop {
  position: absolute;
  top: 40px;
  right: 0;
  z-index: 20;
  width: 278px;
  padding: var(--sy-space-2);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
  box-shadow: var(--sy-shadow-window-2);
  animation: sy-in 0.14s ease-out;
}

.card__menu-head {
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 9px var(--sy-space-4) var(--sy-space-1);
}

.card__menu-tag {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.card__menu-dot {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-warn);
}

.card__menu-text {
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.card__menu-rule {
  height: 1px;
  margin: 7px var(--sy-space-2) 5px;
  border: none;
  background: var(--sy-border);
}

.card__menu-foot {
  padding: 0 var(--sy-space-4) var(--sy-space-3);
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--sy-text-3);
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

/*
 * Код и заметка делят одну строку, но не поровну: коду хватает 280px под шесть
 * цифр с отсчётом, а заметке нужно всё остальное (`Прототип:1651`).
 */
.card__grid--secrets {
  grid-template-columns: 280px minmax(0, 1fr);
  gap: var(--sy-space-6);
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

/* Дополнительные адреса: чипы вместо строк — их бывает пять на одну запись. */
.card__chips {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin-top: var(--sy-space-2);
}

.card__chip {
  max-width: 100%;
  height: 26px;
  padding: 0 9px;
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-xs);
  background: var(--sy-bg-0);
  color: var(--sy-text-2);
  font-family: var(--sy-font-mono);
  font-size: 11.5px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.card__chip:hover {
  border-color: var(--sy-accent);
  color: var(--sy-text);
}

.card__chip:focus-visible,
.card__more:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.card__more {
  align-self: flex-start;
  height: 26px;
  margin-top: var(--sy-space-2);
  padding: 0 9px;
  border: 1px dashed var(--sy-accent-border);
  border-radius: var(--sy-radius-xs);
  background: transparent;
  color: var(--sy-accent);
  font-family: var(--sy-font-mono);
  font-size: 11.5px;
  cursor: pointer;
}

.card__more:hover {
  background: var(--sy-accent-quiet);
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
  margin-bottom: var(--sy-space-5);
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  color: var(--sy-text-3);
}

.card__confirm-text {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}
</style>
