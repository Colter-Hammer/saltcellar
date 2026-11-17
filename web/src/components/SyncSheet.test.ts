import { VueQueryPlugin } from '@tanstack/vue-query'
import { mount, RouterLinkStub } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import type { SyncState } from '@/api/bank'
import { quietStates, spot, syncProblems } from '@/lib/syncHealth'
import { account, sync } from '@/lib/syncFixtures'
import SyncSheetBody from './SyncSheetBody.vue'
import SyncSpot from './SyncSpot.vue'

const formatting = { locale: 'en-US', currency: 'USD' } as never
const state: SyncState = {
  time: '06:13',
  keep_raw_responses: false,
  request_cap: 12,
  requests_left: 9,
  last_synced_at: '2026-11-17T06:13:00Z',
  cooldown_until: null,
  limit_until: null,
}
const now = new Date('2026-11-17T12:00:00Z')

function sheet(accounts: ReturnType<typeof account>[], paused = false, extra = {}) {
  return mount(SyncSheetBody, {
    props: {
      paused,
      pausedMessage: paused ? 'Paste a new setup token.' : null,
      sync: state,
      problems: syncProblems(accounts),
      quiet: quietStates(accounts),
      now,
      timeZone: 'UTC',
      formatting,
      syncing: false,
      failure: '',
      ...extra,
    },
    global: { stubs: { RouterLink: RouterLinkStub }, plugins: [VueQueryPlugin] },
  })
}

const connection = (paused: boolean) => ({
  label: 'SimpleFIN Bridge',
  status: paused ? ('token_revoked' as const) : ('ok' as const),
  paused,
  last_sync: null,
  banks: [],
})

describe('SyncSpot', () => {
  const states: [string, number, boolean, string][] = [
    ['all good', 0, false, '6:13 AM'],
    ['1 problem', 1, false, '1 sync problem'],
    ['2 problems', 2, false, '2 sync problems'],
    ['paused', 0, true, 'Sync paused'],
  ]
  for (const variant of ['pill', 'button'] as const) {
    for (const [name, problems, paused, short] of states) {
      it(`${variant}, ${name}`, () => {
        const s = spot(connection(paused), problems, state.last_synced_at, 'UTC', formatting)
        if (!s) throw new Error('no spot')
        const wrapper = mount(SyncSpot, { props: { spot: s, variant } })
        const text = wrapper.text()
        expect(text).toBe(variant === 'pill' ? short : s.long)
        if (variant === 'button' && !paused && problems === 0) expect(text).toBe('Synced 6:13 AM')
        expect(wrapper.attributes('data-problem')).toBe(String(problems > 0 || paused))
      })
    }
  }
})

describe('SyncSheetBody', () => {
  it('all good', () => {
    const wrapper = sheet([account(1, { sync: sync('ok') })])
    expect(wrapper.get('[data-testid=sync-last-line]').text()).toBe(
      'Synced 6:13 AM · syncs daily around 6 AM',
    )
    expect(wrapper.text()).toContain('All good.')
    expect(wrapper.get('[data-testid=sync-requests]').text()).toBe('9 of 12 requests left today')
  })

  it('stale only: listed, uncounted, no problem', () => {
    const wrapper = sheet([account(1, { sync: sync('stale') })])
    expect(wrapper.findAll('[data-testid=sync-problem]')).toHaveLength(0)
    expect(wrapper.get('[data-testid=sync-quiet]').text()).toContain('Demo 1: message stale')
  })

  it('a problem with the fix it names', () => {
    const wrapper = sheet([account(1, { sync: sync('needs_reconnecting', true) })])
    const problem = wrapper.get('[data-testid=sync-problem]')
    expect(problem.text()).toContain('Demo 1')
    expect(problem.text()).toContain('Needs reconnecting')
    const fix = problem.get('a')
    expect(fix.text()).toBe('Fix at SimpleFIN ↗')
    expect(fix.attributes('href')).toMatch(/^https:\/\/bridge\.simplefin\.org\//)
  })

  it('drift fix goes to the suspect rows', () => {
    const base = account(5)
    const flag = { drift_minor: 100, since: '2026-11-12', suspect_rows: [] }
    const wrapper = sheet([account(5, { drift: { ...base.drift, flag } })])
    const link = wrapper.getComponent(RouterLinkStub)
    expect(link.text()).toBe('See suspect rows')
    expect(link.props('to')).toBe('/accounts/5?suspect=1')
  })

  it('two problems', () => {
    const wrapper = sheet([
      account(1, { sync: sync('needs_reconnecting', true) }),
      account(2, { sync: sync('needs_attention') }),
    ])
    expect(wrapper.findAll('[data-testid=sync-problem]')).toHaveLength(2)
  })

  it('paused: Paste new token goes to Settings, no Sync now', () => {
    const wrapper = sheet([], true)
    const link = wrapper
      .findAllComponents(RouterLinkStub)
      .find((l) => l.text() === 'Paste new token')
    expect(link?.props('to')).toBe('/settings/bank')
    expect(wrapper.findAll('button')).toHaveLength(0)
  })

  it('Sync now waits out its cooldown and emits when free', async () => {
    const wrapper = sheet([], false, {
      sync: {
        ...state,
        cooldown_until: '2026-11-17T12:05:00Z',
        last_synced_at: '2026-11-17T11:50:00Z',
      },
    })
    expect(wrapper.get('button').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid=sync-wait]').text()).toBe('Synced 10 min ago')
    const free = sheet([])
    await free.get('button').trigger('click')
    expect(free.emitted('syncNow')).toHaveLength(1)
  })
})
