<script setup lang="ts">
// The log-in page (docs/spec/m1-ledger-import.md#login-and-profile): every visit without a live
// session lands here, and goes back where it was afterwards.
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { logIn } from '@/api/session'
import FormField from '@/components/FormField.vue'

const route = useRoute()
const router = useRouter()

const username = ref('')
const password = ref('')
const error = ref('')

/** Where to go once logged in: `next`, if it's a screen in this app. */
function next(): string {
  const { next } = route.query
  return typeof next === 'string' && next.startsWith('/') && !next.startsWith('//')
    ? next
    : '/transactions'
}

async function submit() {
  error.value = ''
  try {
    await logIn(username.value, password.value)
  } catch (failure) {
    error.value = failure instanceof Error ? failure.message : String(failure)
    password.value = ''
    return
  }
  await router.replace(next())
}
</script>

<template>
  <main class="mx-auto flex min-h-dvh max-w-md flex-col gap-6 p-4 lg:pt-16">
    <h1 class="text-xl font-semibold">Log in</h1>

    <form class="flex flex-col gap-4" @submit.prevent="submit">
      <FormField
        v-model="username"
        name="username"
        label="Username"
        autocomplete="username"
        autocapitalize="none"
        spellcheck="false"
        required
      />
      <FormField
        v-model="password"
        name="password"
        label="Password"
        type="password"
        autocomplete="current-password"
        required
      />
      <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Log in
      </button>
    </form>
  </main>
</template>
