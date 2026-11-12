<script setup lang="ts">
// Settings › Bank connection & sync (docs/spec/m2-bank-data.md#settings--bank-connection--sync):
// paste a setup token to claim it, replace it, or disconnect; Sync now with its cooldown and the
// requests left, the daily sync time, keeping raw responses, and Recent syncs. The access URL it
// yields is never shown; the server keeps it sealed.
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  useBankConnection,
  useChangeSyncSettings,
  useClaimSetupToken,
  useDisconnectBank,
  useSyncHistory,
  useSyncNow,
} from '@/api/bank'
import { ApiError } from '@/api/client'
import { useHousehold } from '@/api/household'
import BankFix from '@/components/BankFix.vue'
import FormField from '@/components/FormField.vue'
import { formatInstant, formatInstantDay, useFormatting } from '@/lib/format'
import {
  accountCount,
  accountProblem,
  bankState,
  runSummary,
  scheduleLine,
  syncNowWait,
  triggerName,
} from '@/lib/sync'

const bank = useBankConnection()
const claim = useClaimSetupToken()
const disconnect = useDisconnectBank()
const syncNow = useSyncNow()
const changeSettings = useChangeSyncSettings()
const history = useSyncHistory()
const household = useHousehold()
const formatting = useFormatting()

const timeZone = computed(() => household.data.value?.timezone ?? 'UTC')
const syncState = computed(() => bank.data.value?.sync ?? null)

/** The clock "Synced 12 min ago" and the waits read, moved on every 15 seconds. */
const now = ref(new Date())
const clock = globalThis.setInterval(() => {
  now.value = new Date()
}, 15_000)
onBeforeUnmount(() => {
  globalThis.clearInterval(clock)
})

const wait = computed(() =>
  syncState.value === null
    ? null
    : syncNowWait(syncState.value, now.value, timeZone.value, formatting.value),
)

const syncTime = ref('')
const timeError = ref('')
const settingsFailure = ref('')
watch(
  () => syncState.value?.time,
  (time) => {
    if (time !== undefined) syncTime.value = time
  },
  { immediate: true },
)

async function saveSettings(keepRaw: boolean) {
  timeError.value = ''
  settingsFailure.value = ''
  try {
    await changeSettings.mutateAsync({ sync_time: syncTime.value, keep_raw_responses: keepRaw })
  } catch (error) {
    if (error instanceof ApiError && error.fields.sync_time) {
      timeError.value = error.fields.sync_time
    } else {
      settingsFailure.value = error instanceof Error ? error.message : 'Something went wrong.'
    }
  }
}

function toggleRaw(event: Event) {
  if (event.target instanceof HTMLInputElement) void saveSettings(event.target.checked)
}

const connection = computed(() => bank.data.value?.connection ?? null)
const keySet = computed(() => bank.data.value?.secret_key_set ?? false)
const lastSync = computed(() => connection.value?.last_sync ?? null)

/** What a paused connection says, and what to do about it. */
const PAUSED: Record<string, string> = {
  token_revoked: 'SimpleFIN turned the access down. Paste a new setup token.',
  unpaid: 'The SimpleFIN subscription is unpaid. Pay it, then paste a new setup token.',
  key_mismatch: 'The secret key changed. Paste a new setup token.',
}

const token = ref('')
/** The form is open for Replace; with no connection, or a paused one, it always is. */
const replacing = ref(false)
const confirmingDisconnect = ref(false)
const tokenError = ref('')
const failure = ref('')

const formOpen = computed(
  () => connection.value === null || connection.value.paused || replacing.value,
)

async function submit() {
  tokenError.value = ''
  failure.value = ''
  try {
    await claim.mutateAsync(token.value)
    token.value = ''
    replacing.value = false
  } catch (error) {
    if (error instanceof ApiError && error.fields.setup_token) {
      tokenError.value = error.fields.setup_token
    } else {
      failure.value = error instanceof Error ? error.message : 'Something went wrong.'
    }
  }
}

function cancelReplace() {
  replacing.value = false
  token.value = ''
  tokenError.value = ''
  failure.value = ''
}

const syncFailure = ref('')

async function sync() {
  syncFailure.value = ''
  try {
    await syncNow.mutateAsync()
  } catch (error) {
    syncFailure.value = error instanceof Error ? error.message : 'Something went wrong.'
  }
}

async function confirmDisconnect() {
  failure.value = ''
  try {
    await disconnect.mutateAsync()
    confirmingDisconnect.value = false
    replacing.value = false
  } catch (error) {
    failure.value = error instanceof Error ? error.message : 'Something went wrong.'
  }
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:max-w-md lg:px-6">
    <p v-if="bank.isError.value" role="alert" class="text-sm text-destructive">
      {{ bank.error.value?.message }}
    </p>

    <template v-if="bank.isSuccess.value">
      <section aria-labelledby="connection-heading" class="flex flex-col gap-3">
        <h2 id="connection-heading" class="font-semibold">Connection</h2>

        <template v-if="connection">
          <template v-if="!connection.paused">
            <p data-testid="connection-state">{{ connection.label }} · connected</p>
            <p v-if="syncState" data-testid="sync-schedule" class="text-sm text-muted-foreground">
              {{ scheduleLine(syncState, formatting) }}
            </p>
            <p v-if="lastSync" data-testid="last-sync" class="text-sm text-muted-foreground">
              Last sync: {{ runSummary(lastSync) }}
            </p>
            <div class="flex flex-col items-start gap-2">
              <button
                type="button"
                class="rounded-md border px-3 py-2 disabled:opacity-50"
                :disabled="syncNow.isPending.value || wait !== null"
                @click="sync"
              >
                {{ syncNow.isPending.value ? 'Syncing…' : 'Sync now' }}
              </button>
              <p v-if="wait" data-testid="sync-wait" class="text-sm text-muted-foreground">
                {{ wait }}
              </p>
              <p v-if="syncFailure" role="alert" class="text-sm text-destructive">
                {{ syncFailure }}
              </p>
            </div>
          </template>
          <template v-else>
            <p data-testid="connection-state">{{ connection.label }} · Sync paused</p>
            <p role="status" class="text-sm text-muted-foreground">
              {{ PAUSED[connection.status] ?? 'Paste a new setup token.' }}
            </p>
          </template>
        </template>
        <p v-else class="text-sm text-muted-foreground">
          Not connected. Bank data arrives through SimpleFIN Bridge.
        </p>

        <p v-if="!keySet" role="alert" data-testid="key-missing" class="text-sm text-destructive">
          Bank sync needs a secret key, and this server has none. Set
          <code>BUDGET_SECRET_KEY</code> to 32 random bytes in base64 (<code
            >openssl rand -base64 32</code
          >), restart the app, and come back to paste a setup token. Nothing is claimed until then.
        </p>

        <form v-else-if="formOpen" class="flex flex-col gap-3" @submit.prevent="submit">
          <FormField
            v-model="token"
            name="setup_token"
            :label="connection ? 'New setup token' : 'Setup token'"
            hint="Get a setup token from SimpleFIN Bridge and paste it here."
            :error="tokenError"
            autocomplete="off"
            spellcheck="false"
            required
          />
          <p v-if="failure" role="alert" class="text-sm text-destructive">{{ failure }}</p>
          <div class="flex flex-wrap gap-2">
            <button
              type="submit"
              class="rounded-md bg-primary px-3 py-2 text-primary-foreground disabled:opacity-50"
              :disabled="claim.isPending.value"
            >
              {{ connection ? 'Replace' : 'Connect' }}
            </button>
            <button
              v-if="replacing"
              type="button"
              class="rounded-md border px-3 py-2"
              @click="cancelReplace"
            >
              Cancel
            </button>
          </div>
        </form>

        <div v-if="connection" class="flex flex-col items-start gap-2">
          <p v-if="failure && !formOpen" role="alert" class="text-sm text-destructive">
            {{ failure }}
          </p>
          <div
            v-if="!replacing && !connection.paused && !confirmingDisconnect"
            class="flex flex-wrap gap-2"
          >
            <button
              v-if="keySet"
              type="button"
              class="rounded-md border px-3 py-2"
              @click="replacing = true"
            >
              Replace setup token…
            </button>
            <button
              type="button"
              class="rounded-md border px-3 py-2"
              @click="confirmingDisconnect = true"
            >
              Disconnect
            </button>
          </div>
          <button
            v-if="connection.paused && !confirmingDisconnect"
            type="button"
            class="rounded-md border px-3 py-2"
            @click="confirmingDisconnect = true"
          >
            Disconnect
          </button>
          <template v-if="confirmingDisconnect">
            <p role="alert">
              Disconnect from SimpleFIN? Syncing stops and the stored access is deleted. Your
              accounts and transactions stay, and become manual accounts.
            </p>
            <div class="flex flex-wrap gap-2">
              <button
                type="button"
                class="rounded-md bg-destructive px-3 py-2 text-destructive-foreground disabled:opacity-50"
                :disabled="disconnect.isPending.value"
                @click="confirmDisconnect"
              >
                Disconnect
              </button>
              <button
                type="button"
                class="rounded-md border px-3 py-2"
                @click="confirmingDisconnect = false"
              >
                Cancel
              </button>
            </div>
          </template>
        </div>
      </section>

      <section
        v-if="connection && connection.banks.length > 0"
        aria-labelledby="banks-heading"
        class="flex flex-col gap-3"
      >
        <h2 id="banks-heading" class="font-semibold">Banks</h2>
        <ul aria-label="Banks" class="flex flex-col divide-y">
          <li
            v-for="(link, index) in connection.banks"
            :key="index"
            data-testid="bank"
            class="flex flex-col gap-1 py-3 text-sm"
          >
            <span class="flex flex-wrap items-baseline justify-between gap-x-3">
              <span class="font-medium">{{ link.name }}</span>
              <span class="text-muted-foreground">{{ accountCount(link.accounts) }}</span>
            </span>
            <span
              :data-state="link.state"
              :class="
                link.state === 'needs_reconnecting' || link.state === 'needs_attention'
                  ? 'font-medium text-destructive'
                  : 'text-muted-foreground'
              "
            >
              {{ bankState(link) }}
            </span>
            <span v-if="link.message" class="text-muted-foreground">{{ link.message }}</span>
            <span v-if="link.last_refreshed_at" class="text-muted-foreground">
              Last refreshed {{ formatInstantDay(link.last_refreshed_at, timeZone, formatting) }}
            </span>
            <BankFix v-if="link.fix && !connection.paused" class="mt-1" />
          </li>
        </ul>
      </section>

      <section
        v-if="connection && syncState"
        aria-labelledby="schedule-heading"
        class="flex flex-col gap-3"
      >
        <h2 id="schedule-heading" class="font-semibold">Schedule</h2>
        <form
          class="flex flex-col gap-3"
          @submit.prevent="saveSettings(syncState.keep_raw_responses)"
        >
          <FormField
            v-model="syncTime"
            name="sync_time"
            label="Daily sync time"
            hint="In the household's timezone. Off the hour is kinder to the bank's servers."
            :error="timeError"
            type="time"
            required
          />
          <div>
            <button
              type="submit"
              class="rounded-md border px-3 py-2 disabled:opacity-50"
              :disabled="changeSettings.isPending.value || syncTime === syncState.time"
            >
              Save time
            </button>
          </div>
        </form>
        <label class="flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            class="mt-0.5"
            :checked="syncState.keep_raw_responses"
            :disabled="changeSettings.isPending.value"
            @change="toggleRaw"
          />
          <span>
            Keep raw bank responses for troubleshooting
            <span class="block text-muted-foreground">
              Kept encrypted for 7 days. Turning this off deletes them.
            </span>
          </span>
        </label>
        <p v-if="settingsFailure" role="alert" class="text-sm text-destructive">
          {{ settingsFailure }}
        </p>
      </section>

      <section v-if="connection" aria-labelledby="runs-heading" class="flex flex-col gap-3">
        <h2 id="runs-heading" class="font-semibold">Recent syncs</h2>
        <p v-if="history.isError.value" role="alert" class="text-sm text-destructive">
          {{ history.error.value?.message }}
        </p>
        <p
          v-else-if="history.data.value && history.data.value.runs.length === 0"
          class="text-sm text-muted-foreground"
        >
          No syncs yet.
        </p>
        <ul v-else-if="history.data.value" aria-label="Recent syncs" class="flex flex-col divide-y">
          <li
            v-for="entry in history.data.value.runs"
            :key="entry.run.id"
            class="flex flex-col gap-0.5 py-2 text-sm"
          >
            <span class="font-medium">
              {{ formatInstant(entry.run.started_at, timeZone, formatting) }} ·
              {{ triggerName(entry.run.trigger) }}
            </span>
            <span class="text-muted-foreground">
              {{ runSummary(entry.run) }} ·
              {{ entry.run.requests === 1 ? '1 request' : `${entry.run.requests} requests` }}
            </span>
            <span
              v-for="account in entry.accounts.filter((a) => !a.ok)"
              :key="account.account_id"
              class="text-destructive"
            >
              {{ accountProblem(account) }}
            </span>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>
