import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import type { Device, DeviceId } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Доверенные устройства (F9, §2.3).
 *
 * ЗАКОН №1: секретов здесь нет и появиться неоткуда — `Device` их не содержит
 * по контракту, а ключи остаются в ядре. Но состав списка — содержимое
 * хранилища («какими устройствами пользуется владелец»), поэтому по событию
 * `locked` он очищается наравне с записями и секциями.
 *
 * Список сознательно НЕ фильтруется от отозванных: отзыв — факт истории
 * хранилища, и человек, отозвавший украденный ноутбук, должен видеть, что тот
 * отозван, а не то, что его никогда не было.
 */
export const useDevicesStore = defineStore('devices', () => {
  const devices = ref<Device[]>([])
  const loading = ref(false)
  /** Был ли хоть один успешный ответ ядра. */
  const loaded = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)

  /** Действующие устройства — те, что продолжают получать обновления. */
  const active = computed(() => devices.value.filter((device) => device.revoked_at === null))

  /** Отозванные (§2.3). Остаются в списке, но доступа больше не имеют. */
  const revoked = computed(() => devices.value.filter((device) => device.revoked_at !== null))

  /** Устройство, на котором открыт UI. Себя отозвать нельзя. */
  const thisDevice = computed(() => devices.value.find((device) => device.is_this_device) ?? null)

  /**
   * Хранилище живёт только на этом устройстве: копий больше нигде нет.
   * Отозванные не считаются — им обновления не приезжают.
   */
  const isAlone = computed(() => active.value.length <= 1)

  let unsubscribe: Unsubscribe | null = null

  function watchCore(): void {
    if (unsubscribe) return
    unsubscribe = useCore().on('locked', () => {
      clear()
    })
  }

  function clear(): void {
    devices.value = []
    loaded.value = false
    error.value = null
  }

  /** Забрать список у ядра, если он ещё не забирался. */
  async function ensure(): Promise<Device[]> {
    if (loaded.value) return devices.value
    return load()
  }

  async function load(): Promise<Device[]> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      devices.value = await useCore().listDevices()
      loaded.value = true
    } catch (cause) {
      devices.value = []
      loaded.value = false
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить список устройств.'
    } finally {
      loading.value = false
    }
    return devices.value
  }

  /**
   * Отозвать доступ (§2.3).
   *
   * Ошибка летит наружу исключением и НЕ пишется в `error`: тот гасит список и
   * показывает сообщение вместо него, а отказ на конкретном устройстве должен
   * быть виден рядом с ним — остальные строки продолжают работать.
   */
  async function revoke(deviceId: DeviceId): Promise<Device> {
    const updated = await useCore().revokeDevice(deviceId)
    devices.value = devices.value.map((device) =>
      device.device_id === updated.device_id ? updated : device,
    )
    return updated
  }

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return {
    devices,
    loading,
    loaded,
    error,
    active,
    revoked,
    thisDevice,
    isAlone,
    ensure,
    load,
    revoke,
    clear,
    dispose,
  }
})
