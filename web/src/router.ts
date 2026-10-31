import { createRouter, type RouteLocationRaw, type Router, type RouterHistory } from 'vue-router'
import { onLoginRequired } from '@/api/client'
import { getSession } from '@/api/session'
import { isSetupDone } from '@/api/setup'
import type { Tab } from '@/lib/nav'
import AccountsView from '@/views/AccountsView.vue'
import AccountView from '@/views/AccountView.vue'
import CategoriesView from '@/views/CategoriesView.vue'
import ImportsView from '@/views/ImportsView.vue'
import ImportView from '@/views/ImportView.vue'
import LoginView from '@/views/LoginView.vue'
import MoreView from '@/views/MoreView.vue'
import NotFoundView from '@/views/NotFoundView.vue'
import ProfileView from '@/views/ProfileView.vue'
import ResetPasswordView from '@/views/ResetPasswordView.vue'
import SettingsView from '@/views/SettingsView.vue'
import SetupView from '@/views/SetupView.vue'
import TransactionsView from '@/views/TransactionsView.vue'
import UndecidedView from '@/views/UndecidedView.vue'

declare module 'vue-router' {
  interface RouteMeta {
    /** The screen's heading. */
    title: string
    /** The phone tab that's current on this screen. */
    tab?: Tab
    /** Shown on its own, without the app's navigation. */
    bare?: boolean
  }
}

export function createAppRouter(history: RouterHistory): Router {
  const router = createRouter({
    history,
    routes: [
      { path: '/', redirect: '/transactions' },
      {
        // The open transaction's detail, if any, is in the address.
        path: '/transactions/:id(\\d+)?',
        component: TransactionsView,
        meta: { title: 'Transactions', tab: 'transactions' },
      },
      { path: '/accounts', component: AccountsView, meta: { title: 'Accounts', tab: 'accounts' } },
      {
        path: '/accounts/:id(\\d+)',
        component: AccountView,
        meta: { title: 'Account', tab: 'accounts' },
      },
      { path: '/more', component: MoreView, meta: { title: 'More', tab: 'more' } },
      {
        path: '/categories',
        component: CategoriesView,
        meta: { title: 'Categories', tab: 'more' },
      },
      {
        path: '/import',
        component: ImportView,
        meta: { title: 'Import from Actual', tab: 'more' },
      },
      {
        // The open row's detail, if any, is in the address.
        path: '/undecided/:id(\\d+)?',
        component: UndecidedView,
        meta: { title: 'From Actual, undecided', tab: 'more' },
      },
      { path: '/settings', component: SettingsView, meta: { title: 'Settings', tab: 'more' } },
      {
        path: '/settings/profile',
        component: ProfileView,
        meta: { title: 'Profile', tab: 'more' },
      },
      {
        path: '/settings/imports',
        component: ImportsView,
        meta: { title: 'Imports', tab: 'more' },
      },
      { path: '/setup', component: SetupView, meta: { title: 'Set up Saltcellar', bare: true } },
      { path: '/login', component: LoginView, meta: { title: 'Log in', bare: true } },
      {
        path: '/reset-password',
        component: ResetPasswordView,
        meta: { title: 'Set a new password', bare: true },
      },
      { path: '/:unknown(.*)*', component: NotFoundView, meta: { title: 'Not found' } },
    ],
  })
  // Until setup is finished, the wizard is the only screen. Once it is, it stays so: the server
  // isn't asked again. Then every screen but the log-in and reset pages needs a login, asked for
  // once and asked again after a trip to either of them. If the server can't be asked, the screen shows anyway,
  // under the server banner.
  let setupDone = false
  let loggedIn = false
  router.beforeEach(async (to) => {
    const toWizard = to.path === '/setup'
    try {
      setupDone ||= await isSetupDone()
    } catch {
      return true
    }
    if (!setupDone) return toWizard || '/setup'
    if (toWizard) return '/transactions'
    if (to.path === '/login' || to.path === '/reset-password') {
      loggedIn = false
      return true
    }
    try {
      loggedIn ||= (await getSession()) !== null
    } catch {
      return true
    }
    return loggedIn || logInFirst(to.fullPath)
  })
  // A call that finds the session gone mid-screen: log in, then back to that screen.
  onLoginRequired(() => {
    const { path, fullPath } = router.currentRoute.value
    if (path !== '/login') void router.push(logInFirst(fullPath))
  })
  return router
}

/** The log-in page, coming back to `next` afterwards. */
function logInFirst(next: string): RouteLocationRaw {
  return { path: '/login', query: { next } }
}
