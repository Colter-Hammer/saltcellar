<script setup lang="ts">
// Settings › Profile (docs/spec/m1-ledger-import.md#login-and-profile): this login's name, the
// locale and week start its dates and numbers follow, its palette and mode
// (docs/spec/visual-system.md#palettes-and-modes), its password, and logging out everywhere.
import { useQueryClient } from '@tanstack/vue-query'
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ApiError } from '@/api/client'
import {
  changePassword,
  logOutEverywhere,
  useChangePalette,
  useChangeProfile,
  useProfile,
} from '@/api/profile'
import FormField from '@/components/FormField.vue'
import SearchSelect from '@/components/SearchSelect.vue'
import Money from '@/components/Money.vue'
import { formatDate, localeWeekStart, useFormatting, weekdayName } from '@/lib/format'
import { localeLabel, localeOptions } from '@/lib/locales'
import { MODES, useAppearance } from '@/theme/appearance'
import { PALETTES, paletteOrDefault } from '@/theme/palettes'

const router = useRouter()
const queryClient = useQueryClient()
const profile = useProfile()
const saveProfile = useChangeProfile()
const formatting = useFormatting()

const browser = globalThis.navigator.language

/** Errors by field name, from the server, or `form` for one that names no field. */
function fieldErrors(error: unknown): Record<string, string> {
  if (error instanceof ApiError && Object.keys(error.fields).length > 0) return { ...error.fields }
  return { form: error instanceof Error ? error.message : String(error) }
}

// ── Name, locale and week start ──

const displayName = ref('')
/** What the locale list holds for the browser's own: the list can't hold ''. */
const BROWSER = 'browser'
/** A BCP 47 tag, or BROWSER. */
const locale = ref(BROWSER)
/** 0–6, or '' for the locale's. */
const weekStart = ref('')
const profileErrors = ref<Record<string, string>>({})
const profileSaved = ref(false)

watch(
  () => profile.data.value,
  (loaded) => {
    if (loaded === undefined) return
    displayName.value = loaded.display_name
    locale.value = loaded.display_locale ?? BROWSER
    weekStart.value = loaded.week_start == null ? '' : String(loaded.week_start)
  },
  { immediate: true },
)

const locales = [
  { value: BROWSER, label: `Browser default (${localeLabel(browser)})` },
  ...localeOptions(browser),
]
/** What the week-start list calls its days: in English, as the whole app is. */
const days = [0, 1, 2, 3, 4, 5, 6].map((day) => ({
  value: String(day),
  label: weekdayName(day, 'en'),
}))
const localeDay = computed(() =>
  weekdayName(localeWeekStart(locale.value === BROWSER ? browser : locale.value), 'en'),
)

/** What the app shows with the saved profile: it changes once the profile is saved. */
const example = computed(() => ({
  date: formatDate('2026-03-01', formatting.value),
  weekStart: weekdayName(formatting.value.weekStart, 'en'),
}))

async function submitProfile() {
  profileErrors.value = {}
  profileSaved.value = false
  try {
    await saveProfile.mutateAsync({
      display_name: displayName.value,
      display_locale: locale.value === BROWSER ? null : locale.value,
      week_start: weekStart.value === '' ? null : Number(weekStart.value),
    })
    profileSaved.value = true
  } catch (error) {
    profileErrors.value = fieldErrors(error)
  }
}

// ── Appearance: applied at once, no save button ──

const changePalette = useChangePalette()
const appearance = useAppearance()
const palette = computed(() => paletteOrDefault(profile.data.value?.palette))
const paletteError = ref('')

async function pickPalette(id: string) {
  paletteError.value = ''
  try {
    await changePalette.mutateAsync(id)
  } catch (error) {
    paletteError.value = error instanceof Error ? error.message : String(error)
  }
}

// ── Password ──

const currentPassword = ref('')
const newPassword = ref('')
const newPasswordAgain = ref('')
const passwordErrors = ref<Record<string, string>>({})
const passwordChanged = ref(false)

async function submitPassword() {
  passwordErrors.value = {}
  passwordChanged.value = false
  try {
    await changePassword({
      current_password: currentPassword.value,
      password: newPassword.value,
      password_again: newPasswordAgain.value,
    })
  } catch (error) {
    passwordErrors.value = fieldErrors(error)
    return
  }
  currentPassword.value = ''
  newPassword.value = ''
  newPasswordAgain.value = ''
  passwordChanged.value = true
}

// ── Log out everywhere ──

const everywhereError = ref('')

async function leaveEverywhere() {
  everywhereError.value = ''
  try {
    await logOutEverywhere()
  } catch (error) {
    everywhereError.value = error instanceof Error ? error.message : String(error)
    return
  }
  queryClient.clear()
  await router.push('/login')
}
</script>

<template>
  <div class="flex flex-col gap-8 p-4 lg:max-w-md lg:px-6">
    <section aria-labelledby="profile-heading" class="flex flex-col gap-4">
      <h2 id="profile-heading" class="font-semibold">Profile</h2>
      <p v-if="profile.isError.value" role="alert" class="text-sm text-destructive">
        {{ profile.error.value?.message }}
      </p>
      <form class="flex flex-col gap-4" @submit.prevent="submitProfile">
        <FormField
          v-model="displayName"
          name="display_name"
          label="Your name"
          autocomplete="name"
          :error="profileErrors.display_name"
          required
        />
        <SearchSelect
          v-model="locale"
          name="display_locale"
          label="Dates and numbers"
          hint="How dates and amounts look to you. The app stays in English."
          :options="locales"
          :error="profileErrors.display_locale"
        />
        <div class="flex flex-col gap-1.5">
          <label for="week_start" class="text-sm font-medium">Weeks start on</label>
          <select
            id="week_start"
            v-model="weekStart"
            name="week_start"
            :aria-invalid="profileErrors.week_start ? true : undefined"
            :aria-describedby="profileErrors.week_start ? 'week_start-error' : undefined"
            class="rounded-md border bg-background px-3 py-2"
          >
            <option value="">{{ localeDay }}, from the locale</option>
            <option v-for="day in days" :key="day.value" :value="day.value">
              {{ day.label }}
            </option>
          </select>
          <p v-if="profileErrors.week_start" id="week_start-error" class="text-sm text-destructive">
            {{ profileErrors.week_start }}
          </p>
        </div>
        <p data-testid="format-example" class="text-sm text-muted-foreground">
          Amounts look like <Money :minor="-123456" class="text-foreground" /> and dates like
          <span class="text-foreground">{{ example.date }}</span
          >; weeks start on {{ example.weekStart }}.
        </p>
        <p v-if="profileErrors.form" role="alert" class="text-sm text-destructive">
          {{ profileErrors.form }}
        </p>
        <p v-if="profileSaved" role="status" class="">Saved.</p>
        <button
          type="submit"
          class="self-start rounded-md bg-primary px-3 py-2 text-primary-foreground"
        >
          Save
        </button>
      </form>
    </section>

    <section aria-labelledby="appearance-heading" class="flex flex-col gap-4">
      <h2 id="appearance-heading" class="font-semibold">Appearance</h2>
      <fieldset class="flex flex-col gap-2">
        <legend class="mb-2 text-sm font-medium">Palette</legend>
        <div class="grid grid-cols-2 gap-2">
          <label
            v-for="option in PALETTES"
            :key="option.id"
            class="flex cursor-pointer flex-col gap-2 rounded-lg border bg-card p-3 has-checked:border-primary has-checked:ring-1 has-checked:ring-ring has-focus-visible:ring-2 has-focus-visible:ring-ring"
          >
            <input
              type="radio"
              name="palette"
              class="sr-only"
              :value="option.id"
              :checked="palette === option.id"
              @change="pickPalette(option.id)"
            />
            <!-- Drawn by the palette's own tokens, light and dark *(agent)*. -->
            <span class="flex gap-1" aria-hidden="true">
              <span
                v-for="chip in ['', 'dark']"
                :key="chip"
                :data-palette="option.id"
                :class="chip"
                class="flex h-8 flex-1 items-center justify-center rounded-md border bg-background"
              >
                <span class="size-3 rounded-full bg-primary" />
              </span>
            </span>
            <span class="text-sm font-medium">{{ option.label }}</span>
            <span class="text-xs text-muted-foreground">{{ option.swatch }}</span>
          </label>
        </div>
      </fieldset>
      <p v-if="paletteError" role="alert" class="text-sm text-destructive">
        {{ paletteError }}
      </p>
      <fieldset class="flex flex-col gap-2">
        <legend class="mb-2 text-sm font-medium">Light or dark</legend>
        <div class="flex self-start rounded-md border p-0.5">
          <label
            v-for="option in MODES"
            :key="option.value"
            class="inline-flex cursor-pointer items-center rounded-sm px-3 py-1.5 has-checked:bg-primary has-checked:text-primary-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
          >
            <input
              type="radio"
              name="mode"
              class="sr-only"
              :value="option.value"
              :checked="appearance.mode.value === option.value"
              @change="appearance.setMode(option.value)"
            />
            {{ option.label }}
          </label>
        </div>
        <p class="text-sm text-muted-foreground">
          On this device. System follows your phone or computer.
        </p>
      </fieldset>
    </section>

    <section aria-labelledby="password-heading" class="flex flex-col gap-4">
      <h2 id="password-heading" class="font-semibold">Password</h2>
      <form class="flex flex-col gap-4" @submit.prevent="submitPassword">
        <FormField
          v-model="currentPassword"
          name="current_password"
          label="Current password"
          type="password"
          autocomplete="current-password"
          :error="passwordErrors.current_password"
          required
        />
        <FormField
          v-model="newPassword"
          name="password"
          label="New password"
          hint="At least 12 characters."
          type="password"
          autocomplete="new-password"
          :error="passwordErrors.password"
          required
        />
        <FormField
          v-model="newPasswordAgain"
          name="password_again"
          label="New password again"
          type="password"
          autocomplete="new-password"
          :error="passwordErrors.password_again"
          required
        />
        <p v-if="passwordErrors.form" role="alert" class="text-sm text-destructive">
          {{ passwordErrors.form }}
        </p>
        <p v-if="passwordChanged" role="status" class="">Password changed.</p>
        <button type="submit" class="self-start rounded-md border px-3 py-2">
          Change password
        </button>
      </form>
    </section>

    <section aria-labelledby="sessions-heading" class="flex flex-col gap-3">
      <h2 id="sessions-heading" class="font-semibold">Sessions</h2>
      <p class="text-sm text-muted-foreground">
        Logs this login out on every phone and computer, this one included.
      </p>
      <p v-if="everywhereError" role="alert" class="text-sm text-destructive">
        {{ everywhereError }}
      </p>
      <button type="button" class="self-start rounded-md border px-3 py-2" @click="leaveEverywhere">
        Log out everywhere
      </button>
    </section>
  </div>
</template>
