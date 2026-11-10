<script setup lang="ts">
// The new accounts prompt at the top of Accounts (docs/spec/m2-bank-data.md#accounts): "N new
// accounts found", each with Track · budgeted, Track · tracking or Ignore; tracking opens its
// form, with balances only and "This is the same as…". Under it, Ignored, each with Start
// tracking…, and the accounts refused for their currency, with why. One form is open at a time
// *(agent)*. Nothing shows when there's nothing to decide.
import { computed, ref } from 'vue'
import type { Account } from '@/api/accounts'
import type { ConnectionAccounts, FeedAccount, TrackFeedAccount } from '@/api/connectionAccounts'
import TrackFeedAccountForm from '@/components/TrackFeedAccountForm.vue'
import { foundLabel } from '@/lib/connectionAccounts'

const props = defineProps<{ prompt: ConnectionAccounts; accounts: readonly Account[] }>()
const emit = defineEmits<{ track: [id: string, track: TrackFeedAccount]; ignore: [id: string] }>()

/** The open form: which account, and the role its button chose. */
const open = ref<{ id: string; role: Account['role'] }>()

function save(account: FeedAccount, track: TrackFeedAccount) {
  open.value = undefined
  emit('track', account.external_account_id, track)
}

const any = computed(
  () => props.prompt.pending.length + props.prompt.ignored.length + props.prompt.refused.length > 0,
)
</script>

<template>
  <div v-if="any" class="flex flex-col gap-4">
    <section
      v-if="prompt.pending.length > 0"
      aria-labelledby="new-accounts"
      class="flex flex-col gap-2 rounded-xl border p-4"
    >
      <h2 id="new-accounts" class="font-medium">{{ foundLabel(prompt.pending.length) }}</h2>
      <p class="text-sm text-muted-foreground">
        Your bank connection reports these. Nothing from them counts until you decide.
      </p>
      <ul class="flex flex-col divide-y">
        <li
          v-for="account in prompt.pending"
          :key="account.external_account_id"
          :aria-label="account.name"
          class="flex flex-col gap-2 py-3"
        >
          <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
            <span class="flex min-w-0 flex-1 flex-col">
              <span class="truncate">{{ account.name }}</span>
              <span v-if="account.institution" class="text-xs text-muted-foreground">
                {{ account.institution }}
              </span>
            </span>
            <div class="flex flex-wrap gap-2 text-sm">
              <button
                type="button"
                class="rounded-md border px-3 py-1.5"
                @click="open = { id: account.external_account_id, role: 'budgeted' }"
              >
                Track · budgeted
              </button>
              <button
                type="button"
                class="rounded-md border px-3 py-1.5"
                @click="open = { id: account.external_account_id, role: 'tracking' }"
              >
                Track · tracking
              </button>
              <button
                type="button"
                class="rounded-md px-3 py-1.5 text-muted-foreground hover:underline"
                @click="emit('ignore', account.external_account_id)"
              >
                Ignore
              </button>
            </div>
          </div>
          <TrackFeedAccountForm
            v-if="open?.id === account.external_account_id"
            :key="`${account.external_account_id}-${open.role}`"
            :account="account"
            :role="open.role"
            :accounts="accounts"
            @save="save(account, $event)"
            @cancel="open = undefined"
          />
        </li>
      </ul>
    </section>

    <section
      v-if="prompt.ignored.length > 0"
      aria-labelledby="ignored-accounts"
      class="flex flex-col gap-2"
    >
      <h2 id="ignored-accounts" class="text-sm font-medium text-muted-foreground">Ignored</h2>
      <ul class="flex flex-col divide-y rounded-xl border px-4">
        <li
          v-for="account in prompt.ignored"
          :key="account.external_account_id"
          :aria-label="account.name"
          class="flex flex-col gap-2 py-3"
        >
          <div class="flex items-center gap-4">
            <span class="flex min-w-0 flex-1 flex-col">
              <span class="truncate">{{ account.name }}</span>
              <span v-if="account.institution" class="text-xs text-muted-foreground">
                {{ account.institution }}
              </span>
            </span>
            <button
              type="button"
              class="rounded-md border px-3 py-1.5 text-sm"
              @click="open = { id: account.external_account_id, role: 'budgeted' }"
            >
              Start tracking…
            </button>
          </div>
          <TrackFeedAccountForm
            v-if="open?.id === account.external_account_id"
            :account="account"
            :role="open.role"
            :accounts="accounts"
            @save="save(account, $event)"
            @cancel="open = undefined"
          />
        </li>
      </ul>
    </section>

    <section
      v-if="prompt.refused.length > 0"
      aria-labelledby="refused-accounts"
      class="flex flex-col gap-2"
    >
      <h2 id="refused-accounts" class="text-sm font-medium text-muted-foreground">
        Can't be tracked
      </h2>
      <ul class="flex flex-col gap-1 text-sm">
        <li v-for="account in prompt.refused" :key="account.external_account_id" role="note">
          {{ account.message }}
        </li>
      </ul>
    </section>
  </div>
</template>
