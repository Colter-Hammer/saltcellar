<script setup lang="ts">
// The set-new-password page a `budget reset-password` link opens
// (docs/spec/m1-ledger-import.md#login-and-profile). The token is in the link's fragment, which
// browsers never send; it's taken from the address bar at once, so it doesn't linger there.
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ApiError } from '@/api/client'
import { checkReset, resetPassword } from '@/api/reset'
import FormField from '@/components/FormField.vue'

const route = useRoute()
const router = useRouter()

const token = route.hash.slice(1)
/** Checking the link; it works; or it doesn't. */
const state = ref<'checking' | 'ready' | 'dead'>('checking')
/** Why it doesn't, or what went wrong on submit. */
const message = ref('')
const errors = ref<Record<string, string>>({})

const password = ref('')
const passwordAgain = ref('')

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function dead(error: unknown): boolean {
  return error instanceof ApiError && error.code === 'reset_link_dead'
}

onMounted(async () => {
  if (route.hash) await router.replace({ path: route.path })
  if (!token) {
    state.value = 'dead'
    message.value = 'This address has no reset link in it. Copy the whole link again.'
    return
  }
  try {
    await checkReset(token)
    state.value = 'ready'
  } catch (error) {
    state.value = dead(error) ? 'dead' : 'ready'
    message.value = describe(error)
  }
})

async function submit() {
  errors.value = {}
  message.value = ''
  try {
    await resetPassword(token, password.value, passwordAgain.value)
  } catch (error) {
    if (dead(error)) state.value = 'dead'
    if (error instanceof ApiError && Object.keys(error.fields).length > 0) {
      errors.value = { ...error.fields }
    } else {
      message.value = describe(error)
    }
    return
  }
  await router.replace('/transactions')
}
</script>

<template>
  <main class="mx-auto flex min-h-dvh max-w-md flex-col gap-6 p-4 lg:pt-16">
    <h1 class="text-xl font-semibold">Set a new password</h1>

    <template v-if="state === 'dead'">
      <p role="alert" class="text-sm text-destructive">{{ message }}</p>
      <p class="text-sm text-muted-foreground">
        A new link comes from <code>budget reset-password</code> on the app's server.
      </p>
      <RouterLink to="/login" class="underline">Log in</RouterLink>
    </template>

    <form v-else-if="state === 'ready'" class="flex flex-col gap-4" @submit.prevent="submit">
      <FormField
        v-model="password"
        name="password"
        label="New password"
        hint="At least 12 characters."
        type="password"
        autocomplete="new-password"
        :error="errors.password"
        required
      />
      <FormField
        v-model="passwordAgain"
        name="password_again"
        label="New password again"
        type="password"
        autocomplete="new-password"
        :error="errors.password_again"
        required
      />
      <p v-if="message" role="alert" class="text-sm text-destructive">{{ message }}</p>
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Set password and log in
      </button>
    </form>
  </main>
</template>
