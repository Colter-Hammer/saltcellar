import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'
import mark from '@/assets/logo/mark.svg?raw'
import AppLogo from './AppLogo.vue'

it('draws mark.svg itself in the palette accent, beside the wordmark as text', () => {
  const logo = mount(AppLogo)
  const drawn = logo.get('[data-testid="logo-mark"]')
  expect(drawn.classes()).toContain('text-primary')
  expect(drawn.attributes('aria-hidden')).toBe('true')
  const shapes = (root: ParentNode) => [...root.querySelectorAll('path')].map((p) => p.outerHTML)
  const file = new DOMParser().parseFromString(mark, 'text/html').body
  expect(shapes(drawn.element)).toEqual(shapes(file))
  expect(shapes(file)).not.toEqual([])
  expect(drawn.get('svg').attributes('fill')).toBe('currentColor')
  expect(logo.text()).toBe('Saltcellar')
})
