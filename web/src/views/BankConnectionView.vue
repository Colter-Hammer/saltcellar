<script setup lang="ts">
// Settings › Bank connection & sync, the connection part
// (docs/spec/m2-bank-data.md#the-connection): paste a setup token to claim it, replace it, or
// disconnect. The access URL it yields is never shown; the server keeps it sealed.
import { computed, ref } from 'vue'
import { useBankConnection, useClaimSetupToken, useDisconnectBank } from '@/api/bank'
import { ApiError } from '@/api/client'
import FormField from '@/components/FormField.vue'

const bank = useBankConnection()
const claim = useClaimSetupToken()
const disconnect = useDisconnectBank()

const connection = computed(() => bank.data.value?.connection ?? null)
const keySet = computed(() => bank.data.value?.secret_key_set ?? false)

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
          <p v-if="!connection.paused" data-testid="connection-state">
            {{ connection.label }} · connected
          </p>
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
    </template>
  </div>
</template>
