import { createRouter, type Router, type RouterHistory } from 'vue-router'
import type { Tab } from '@/lib/nav'
import AccountsView from '@/views/AccountsView.vue'
import CategoriesView from '@/views/CategoriesView.vue'
import ImportView from '@/views/ImportView.vue'
import MoreView from '@/views/MoreView.vue'
import NotFoundView from '@/views/NotFoundView.vue'
import SettingsView from '@/views/SettingsView.vue'
import TransactionsView from '@/views/TransactionsView.vue'

declare module 'vue-router' {
  interface RouteMeta {
    /** The screen's heading. */
    title: string
    /** The phone tab that's current on this screen. */
    tab?: Tab
  }
}

export function createAppRouter(history: RouterHistory): Router {
  return createRouter({
    history,
    routes: [
      { path: '/', redirect: '/transactions' },
      {
        path: '/transactions',
        component: TransactionsView,
        meta: { title: 'Transactions', tab: 'transactions' },
      },
      { path: '/accounts', component: AccountsView, meta: { title: 'Accounts', tab: 'accounts' } },
      { path: '/more', component: MoreView, meta: { title: 'More', tab: 'more' } },
      { path: '/categories', component: CategoriesView, meta: { title: 'Categories', tab: 'more' } },
      { path: '/import', component: ImportView, meta: { title: 'Import from Actual', tab: 'more' } },
      { path: '/settings', component: SettingsView, meta: { title: 'Settings', tab: 'more' } },
      { path: '/:unknown(.*)*', component: NotFoundView, meta: { title: 'Not found' } },
    ],
  })
}
