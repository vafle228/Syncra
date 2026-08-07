<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'

import { SyButton, SyInput, SyThemeToggle } from '@/components/ui'
import { usePasswordStrength } from '@/composables/usePasswordStrength'
import { useVaultStore } from '@/stores/useVaultStore'

/**
 * Первичная инициализация хранилища (F3, §3.9).
 *
 * Три шага: объяснить модель «нет сервера» → задать мастер-пароль → показать,
 * что дальше. Ключи генерирует ядро — UI отправляет только введённый пароль
 * и сразу его отпускает (Закон №1); в стор он не попадает.
 *
 * Ветка «у меня уже есть Syncra на другом устройстве» ведёт в pairing — это F8,
 * здесь она обозначена как недоступная, а не притворяется работающей.
 */

const router = useRouter()
const vault = useVaultStore()

const step = ref<0 | 1 | 2>(0)
const masterPassword = ref('')
const { level, label, acceptable } = usePasswordStrength(masterPassword)

const promises = [
  {
    title: 'Хранилище лежит на этом компьютере',
    body: 'Не в облаке и не у нас: «у нас» в этой схеме не существует. Файл зашифрован и находится в вашей папке.',
  },
  {
    title: 'Аккаунта и почты не будет',
    body: 'Регистрироваться негде. Второе устройство вы не «входите», а знакомите с этим — по QR, без интернета.',
  },
  {
    title: 'Пароль знаете только вы',
    body: 'Сбросить его невозможно, потому что копии у нас нет. Это обратная сторона того, что данные никуда не уходят.',
  },
]

const canCreate = computed(() => acceptable.value && !vault.busy)

async function create(): Promise<void> {
  if (!canCreate.value) return

  const attempt = masterPassword.value
  // Пароль ушёл в ядро — в компоненте он больше не нужен.
  masterPassword.value = ''

  try {
    await vault.initVault(attempt)
    step.value = 2
  } catch {
    // Сообщение ядра уже в `vault.error`; ничего не логируем.
    step.value = 1
  }
}

function enter(): void {
  void router.push({ name: 'home' })
}
</script>

<template>
  <main class="setup">
    <div class="setup__chrome">
      <span class="setup__chrome-title">Syncra — знакомство</span>
      <SyThemeToggle />
    </div>

    <div class="setup__body">
      <nav class="setup__steps" aria-label="Шаги создания хранилища">
        <span class="setup__steps-caption">Шаги</span>
        <span
          v-for="(name, index) in ['Как это работает', 'Мастер-пароль', 'Готово']"
          :key="name"
          class="setup__step"
          :class="{
            'setup__step--current': step === index,
            'setup__step--done': step > index,
          }"
        >
          <span class="setup__step-number">{{ index + 1 }}</span>
          {{ name }}
        </span>

        <span class="setup__steps-foot">
          <span class="setup__steps-caption">Уже пользуетесь Syncra?</span>
          <span class="setup__steps-hint">
            Создайте хранилище здесь и познакомьте устройства по QR — на экране «Устройства».
            Перенос уже существующего хранилища на чистое устройство появится позже.
          </span>
        </span>
      </nav>

      <section v-if="step === 0" class="setup__pane">
        <h1 class="setup__title">Здесь не нужно заводить аккаунт</h1>
        <p class="setup__lead">
          Syncra не хранит ваши пароли у себя, потому что у Syncra нет «у себя». Хранилище лежит на
          этом компьютере; на другие устройства оно попадает только тогда, когда вы их познакомите.
        </p>

        <ul class="setup__promises">
          <li v-for="promise in promises" :key="promise.title" class="setup__promise">
            <span class="setup__promise-dot" aria-hidden="true" />
            <span>
              <span class="setup__promise-title">{{ promise.title }}</span>
              <span class="setup__promise-body">{{ promise.body }}</span>
            </span>
          </li>
        </ul>

        <div class="setup__actions">
          <SyButton variant="primary" size="lg" @click="step = 1">Создать хранилище</SyButton>
        </div>
      </section>

      <section v-else-if="step === 1" class="setup__pane">
        <h1 class="setup__title">Мастер-пароль — единственный ключ</h1>
        <p class="setup__lead">
          Им шифруется всё хранилище. Мы не сможем его сбросить: у нас нет вашей копии. Придумайте
          фразу из нескольких слов — её проще помнить и труднее подобрать.
        </p>

        <div class="setup__field">
          <SyInput
            v-model="masterPassword"
            label="Мастер-пароль"
            type="password"
            autocomplete="new-password"
            autofocus
            placeholder="например: рыжий трамвай у моста"
            :error="vault.error"
            @submit="create"
          />

          <div class="setup__strength" aria-hidden="true">
            <span
              v-for="bar in 4"
              :key="bar"
              class="setup__strength-bar"
              :class="{ 'setup__strength-bar--on': level >= bar }"
            />
          </div>
          <div class="setup__strength-row">
            <span class="setup__strength-label">{{ label }}</span>
            <span class="setup__strength-count">{{ masterPassword.length }} символов</span>
          </div>
        </div>

        <div class="setup__warning">
          <span class="setup__warning-dot" aria-hidden="true" />
          <span>
            <span class="setup__warning-title">Восстановления не будет</span>
            <span class="setup__warning-body">
              Это обратная сторона того, что пароли никуда не уходят. Забытый мастер-пароль
              восстановить нечем: почты и сервера в этой схеме нет.
            </span>
          </span>
        </div>

        <div class="setup__actions">
          <SyButton
            variant="primary"
            size="lg"
            :disabled="!canCreate"
            :loading="vault.busy"
            @click="create"
          >
            Создать хранилище
          </SyButton>
          <SyButton size="lg" :disabled="vault.busy" @click="step = 0">Назад</SyButton>
        </div>
      </section>

      <section v-else class="setup__pane">
        <h1 class="setup__title">Хранилище создано. Оно только у вас</h1>
        <p class="setup__lead">
          Одна копия — это одна точка отказа. Второе устройство одновременно и удобство, и резерв:
          сломается ноутбук — хранилище останется в телефоне.
        </p>

        <ul class="setup__promises">
          <li class="setup__promise">
            <span class="setup__promise-dot" aria-hidden="true" />
            <span>
              <span class="setup__promise-title">Добавить телефон — по QR</span>
              <span class="setup__promise-body">
                На экране «Устройства» одно показывает код, второе его читает. Код живёт несколько
                минут, показывается локально и не требует интернета.
              </span>
            </span>
          </li>
          <li class="setup__promise">
            <span class="setup__promise-dot" aria-hidden="true" />
            <span>
              <span class="setup__promise-title">Быстрый вход — PIN и биометрия</span>
              <span class="setup__promise-body">
                Они не заменяют мастер-пароль, а открывают уже расшифрованное хранилище в текущем
                сеансе. Тоже впереди.
              </span>
            </span>
          </li>
        </ul>

        <div class="setup__actions">
          <SyButton variant="primary" size="lg" @click="enter">Открыть Syncra</SyButton>
        </div>
      </section>
    </div>
  </main>
</template>

<style scoped>
.setup {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  background: var(--sy-bg-1);
}

.setup__chrome {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  height: 48px;
  padding: 0 var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.setup__chrome-title {
  font-size: 12px;
  color: var(--sy-text-2);
}

.setup__body {
  flex: 1;
  display: grid;
  grid-template-columns: 300px minmax(0, 1fr);
}

.setup__steps {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
  padding: var(--sy-space-8) var(--sy-space-8);
  border-right: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.setup__steps-caption {
  display: block;
  padding-bottom: var(--sy-space-5);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.setup__step {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5);
  border: 1px solid transparent;
  border-radius: 9px;
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
}

.setup__step--current {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-text);
  font-weight: var(--sy-weight-medium);
}

.setup__step--done {
  color: var(--sy-text-3);
}

.setup__step-number {
  display: grid;
  place-items: center;
  flex: none;
  width: 22px;
  height: 22px;
  border: 1px solid var(--sy-border);
  border-radius: 7px;
  font-family: var(--sy-font-mono);
  font-size: 11px;
}

.setup__step--current .setup__step-number {
  border-color: var(--sy-accent-border);
  color: var(--sy-accent);
}

.setup__steps-foot {
  margin-top: auto;
  padding-top: var(--sy-space-6);
  border-top: 1px solid var(--sy-border);
}

.setup__steps-hint {
  display: block;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.setup__pane {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-8);
  max-width: 640px;
  padding: var(--sy-space-10) var(--sy-space-10);
}

.setup__title {
  font-size: 27px;
  line-height: 1.2;
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.02em;
}

.setup__lead {
  font-size: 14.5px;
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.setup__promises {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  margin: 0;
  padding: 0;
  list-style: none;
}

.setup__promise {
  display: flex;
  gap: var(--sy-space-6);
  align-items: flex-start;
  padding: var(--sy-space-6) var(--sy-space-7);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.setup__promise-dot {
  flex: none;
  width: 8px;
  height: 8px;
  margin-top: 5px;
  border-radius: 3px;
  background: var(--sy-accent);
}

.setup__promise-title {
  display: block;
  font-size: 14px;
  font-weight: var(--sy-weight-semibold);
}

.setup__promise-body {
  display: block;
  padding-top: var(--sy-space-1);
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.setup__field {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
}

.setup__strength {
  display: flex;
  gap: var(--sy-space-2);
  height: 6px;
}

.setup__strength-bar {
  flex: 1;
  border-radius: 3px;
  background: var(--sy-surface-2);
}

.setup__strength-bar--on {
  background: var(--sy-accent);
}

.setup__strength-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
}

.setup__strength-label {
  font-size: 12.5px;
  color: var(--sy-text-2);
}

.setup__strength-count {
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-3);
}

.setup__warning {
  display: flex;
  gap: var(--sy-space-6);
  align-items: flex-start;
  padding: var(--sy-space-6) var(--sy-space-7);
  border: 1px solid var(--sy-warn);
  border-radius: var(--sy-radius);
  background: var(--sy-warn-quiet);
}

.setup__warning-dot {
  flex: none;
  width: 7px;
  height: 7px;
  margin-top: 6px;
  border-radius: 50%;
  background: var(--sy-warn);
}

.setup__warning-title {
  display: block;
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.setup__warning-body {
  display: block;
  padding-top: var(--sy-space-1);
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.setup__actions {
  display: flex;
  gap: var(--sy-space-4);
  margin-top: auto;
}

@media (max-width: 900px) {
  .setup__body {
    grid-template-columns: minmax(0, 1fr);
  }

  .setup__steps {
    display: none;
  }

  .setup__pane {
    padding: var(--sy-space-8) var(--sy-space-6);
  }
}
</style>
