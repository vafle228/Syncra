import { beforeEach, describe, it, expect } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount } from '@vue/test-utils'

import App from '../App.vue'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('App', () => {
  it('монтируется и отдаёт выход роутера', () => {
    const wrapper = mount(App, {
      global: { stubs: { RouterView: { template: '<div data-test="router-view" />' } } },
    })

    expect(wrapper.find('[data-test="router-view"]').exists()).toBe(true)
  })
})
