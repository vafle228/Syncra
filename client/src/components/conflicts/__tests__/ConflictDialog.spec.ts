import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import type { RecordConflict } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_RECORD_GITHUB, type MockCoreClient } from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'

import ConflictDialog from '../ConflictDialog.vue'

let core: MockCoreClient
let conflict: RecordConflict

/** Диалог телепортируется в body — искать его надо там же. */
async function mountDialog() {
  const wrapper = mount(ConflictDialog, { props: { conflict }, attachTo: document.body })
  await flushPromises()
  return wrapper
}

function dialogText(): string {
  return document.body.querySelector('.sy-modal__dialog')?.textContent ?? ''
}

function actionButton(index: number): HTMLButtonElement {
  const buttons = document.body.querySelectorAll<HTMLButtonElement>('.sy-modal__actions button')
  return buttons[index]!
}

/** Кнопка выбора — последняя в подвале диалога. */
function chooseButton(): HTMLButtonElement {
  const buttons = document.body.querySelectorAll<HTMLButtonElement>('.sy-modal__actions button')
  return buttons[buttons.length - 1]!
}

async function pickSide(index: 0 | 1): Promise<void> {
  document.body.querySelectorAll<HTMLInputElement>('.conflict__radio')[index]!.click()
  await flushPromises()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)

  await useRecordsStore().load()
  const conflicts = await useConflictsStore().load()
  conflict = conflicts[0]!
})

afterEach(() => {
  document.body.innerHTML = ''
  core.control.dispose()
  setCoreClient(null)
})

describe('полный diff (§5.5)', () => {
  it('показывает обе версии, их устройства и какая из них позже', async () => {
    const wrapper = await mountDialog()

    const text = dialogText()
    expect(text).toContain('Этот компьютер')
    expect(text).toContain('iPhone 14')
    expect(text).toContain('позже')
    expect(text).toContain('раньше')
    expect(text).toContain('отличаются: адреса · пароль · заметки')

    wrapper.unmount()
  })

  it('не выводит ни одного секрета, пока их не открыли (Закон №1)', async () => {
    const wrapper = await mountDialog()

    expect(document.body.innerHTML).not.toContain('mock-github-pw')
    expect(document.body.innerHTML).not.toContain('Recovery codes')
    // Вместо значения — маска фиксированной длины.
    expect(dialogText()).toContain('••••••••••')

    wrapper.unmount()
  })

  it('открывает пару значений по явному нажатию', async () => {
    const wrapper = await mountDialog()

    const reveal = document.body.querySelectorAll<HTMLButtonElement>('.conflict__reveal')
    // Кнопка есть только у разошедшихся секретов: пароль и заметки.
    expect(reveal).toHaveLength(2)
    reveal[0]!.click()
    await flushPromises()

    const text = dialogText()
    expect(text).toContain('mock-github-pw')
    expect(text).toContain('mock-github-pw-phone')

    wrapper.unmount()
  })
})

describe('выбор версии', () => {
  it('ничего не выбрано заранее — даже более свежая версия', async () => {
    const wrapper = await mountDialog()

    expect(chooseButton().textContent?.trim()).toBe('Выберите версию')
    expect(chooseButton().disabled).toBe(true)

    wrapper.unmount()
  })

  it('выбранная колонка называется в кнопке и уходит в ядро целиком', async () => {
    const wrapper = await mountDialog()

    await pickSide(1)
    expect(chooseButton().textContent).toContain('iPhone 14')

    chooseButton().click()
    await flushPromises()

    expect(await core.listConflicts()).toHaveLength(0)
    expect((await core.getSecret(MOCK_RECORD_GITHUB)).password).toBe('mock-github-pw-phone')
    const github = useRecordsStore().records.find(
      (record) => record.record_id === MOCK_RECORD_GITHUB,
    )
    expect(github?.urls).toEqual(['github.com', 'gist.github.com'])
    expect(wrapper.emitted('close')).toBeTruthy()

    wrapper.unmount()
  })

  it('показывает отказ ядра и не закрывается', async () => {
    const wrapper = await mountDialog()
    await pickSide(0)

    core.control.failNext('INTERNAL', 'Хранилище занято.')
    chooseButton().click()
    await flushPromises()

    expect(dialogText()).toContain('Хранилище занято.')
    expect(wrapper.emitted('close')).toBeFalsy()
    expect(await core.listConflicts()).toHaveLength(1)

    wrapper.unmount()
  })

  it('«Решить позже» закрывает диалог, ничего не решая', async () => {
    const wrapper = await mountDialog()

    expect(actionButton(0).textContent?.trim()).toBe('Решить позже')
    actionButton(0).click()
    await flushPromises()

    expect(wrapper.emitted('close')).toBeTruthy()
    expect(await core.listConflicts()).toHaveLength(1)

    wrapper.unmount()
  })
})
