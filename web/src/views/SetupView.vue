<script setup lang="ts">
// The setup wizard (docs/spec/m1-ledger-import.md#setup-wizard): all the app shows until it's done.
// Setup code → currency and timezone → first login; the last step commits everything at once.
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { ApiError } from '@/api/client'
import { checkSetupCode, finishSetup, getCurrencies } from '@/api/setup'
import FormField from '@/components/FormField.vue'
import SearchSelect, { type Option } from '@/components/SearchSelect.vue'
import { defaultCurrency } from '@/lib/currency'

type Step = 'code' | 'household' | 'login'

const router = useRouter()
const step = ref<Step>('code')
/** Errors by field name, from the server. */
const errors = ref<Record<string, string>>({})

const code = ref('')

const locale = globalThis.navigator.language
const currencyNames = new Intl.DisplayNames([locale], { type: 'currency' })
const currencies = ref<Option[]>([])
const currency = ref('')
const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone
const timezones: Option[] = [...new Set([browserZone, ...Intl.supportedValuesOf('timeZone')])]
  .sort()
  .map((zone) => ({ value: zone, label: zone }))
const timezone = ref(browserZone)

const displayName = ref('')
const username = ref('')
const password = ref('')
const passwordAgain = ref('')

function fail(error: unknown, field: string) {
  if (error instanceof ApiError && Object.keys(error.fields).length > 0) {
    errors.value = { ...error.fields }
  } else {
    errors.value = { [field]: error instanceof Error ? error.message : String(error) }
  }
}

async function submitCode() {
  errors.value = {}
  try {
    await checkSetupCode(code.value)
    const codes = await getCurrencies()
    currencies.value = codes.map((c) => ({ value: c, label: `${currencyNames.of(c) ?? c} (${c})` }))
    currency.value = defaultCurrency(locale, codes)
    step.value = 'household'
  } catch (error) {
    fail(error, 'code')
  }
}

function submitHousehold() {
  // The login step's errors stay, for when the server sent the person back here from it.
  errors.value = Object.fromEntries(
    Object.entries(errors.value).filter(([field]) => field !== 'currency' && field !== 'timezone'),
  )
  step.value = 'login'
}

async function submitLogin() {
  errors.value = {}
  try {
    await finishSetup({
      code: code.value,
      currency: currency.value,
      timezone: timezone.value,
      display_name: displayName.value,
      username: username.value,
      password: password.value,
      password_again: passwordAgain.value,
    })
  } catch (error) {
    // A restart since the first step prints a new code; a wrong one is answered on that step.
    const codeProblem =
      error instanceof ApiError && ['setup_code_wrong', 'too_many_attempts'].includes(error.code)
    fail(error, codeProblem ? 'code' : 'form')
    if (codeProblem) step.value = 'code'
    else if ('currency' in errors.value || 'timezone' in errors.value) step.value = 'household'
    return
  }
  await router.push('/transactions')
}
</script>

<template>
  <main class="mx-auto flex min-h-dvh max-w-md flex-col gap-6 p-4 lg:pt-16">
    <h1 class="text-xl font-semibold">Set up Saltcellar</h1>

    <form v-if="step === 'code'" class="flex flex-col gap-4" @submit.prevent="submitCode">
      <FormField
        v-model="code"
        name="code"
        label="Setup code"
        hint="Enter the setup code from the app's log."
        :error="errors.code"
        autocomplete="off"
        autocapitalize="characters"
        spellcheck="false"
        required
      />
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Continue
      </button>
    </form>

    <form
      v-else-if="step === 'household'"
      class="flex flex-col gap-4"
      @submit.prevent="submitHousehold"
    >
      <SearchSelect
        v-model="currency"
        name="currency"
        label="Currency"
        hint="Every amount is in this currency. Type a name or code to search."
        :options="currencies"
        :error="errors.currency"
      />
      <SearchSelect
        v-model="timezone"
        name="timezone"
        label="Timezone"
        hint="It decides when a day starts."
        :options="timezones"
        :error="errors.timezone"
      />
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Continue
      </button>
    </form>

    <form v-else class="flex flex-col gap-4" @submit.prevent="submitLogin">
      <FormField
        v-model="displayName"
        name="display_name"
        label="Your name"
        :error="errors.display_name"
        autocomplete="name"
        required
      />
      <FormField
        v-model="username"
        name="username"
        label="Username"
        :error="errors.username"
        autocomplete="username"
        autocapitalize="none"
        spellcheck="false"
        required
      />
      <FormField
        v-model="password"
        name="password"
        label="Password"
        hint="At least 12 characters."
        :error="errors.password"
        type="password"
        autocomplete="new-password"
        required
      />
      <FormField
        v-model="passwordAgain"
        name="password_again"
        label="Password again"
        :error="errors.password_again"
        type="password"
        autocomplete="new-password"
        required
      />
      <p v-if="errors.form" role="alert" class="text-sm text-destructive">{{ errors.form }}</p>
      <button type="submit" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
        Finish setup
      </button>
    </form>
  </main>
</template>
