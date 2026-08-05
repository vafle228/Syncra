import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'

import App from '../App.vue'

describe('App', () => {
  it('монтируется и отдаёт выход роутера', () => {
    const wrapper = mount(App, {
      global: { stubs: { RouterView: { template: '<div data-test="router-view" />' } } },
    })

    expect(wrapper.find('[data-test="router-view"]').exists()).toBe(true)
  })
})
