<script setup lang="ts">
// What the sync sheet shows (docs/spec/m2-bank-data.md#sync-health-in-the-ui): the last-sync line,
// each problem with its fix, quiet stale states, Sync now with its wait and the requests left, and
// a link to the run history.
import type { SyncState } from '@/api/bank'
import BankFix from '@/components/BankFix.vue'
import type { Formatting } from '@/lib/format'
import { formatAroundHour, formatInstantTime } from '@/lib/format'
import { syncNowWait } from '@/lib/sync'
import type { QuietState, SyncProblem } from '@/lib/syncHealth'

const props = defineProps<{
  paused: boolean
  /** The paused connection's one sentence. */
  pausedMessage: string | null
  sync: SyncState
  problems: SyncProblem[]
  quiet: QuietState[]
  now: Date
  timeZone: string
  formatting: Formatting
  syncing: boolean
  failure: string
}>()
defineEmits<{ syncNow: []; navigate: [] }>()

const lastLine = () => {
  const at = props.sync.last_synced_at
  const when =
    at === null
      ? 'Not synced yet'
      : `Synced ${formatInstantTime(at, props.timeZone, props.formatting)}`
  return `${when} · syncs daily around ${formatAroundHour(props.sync.time, props.formatting)}`
}
const wait = () => syncNowWait(props.sync, props.now, props.timeZone, props.formatting)
</script>

<template>
  <div class="flex flex-col gap-4 text-sm">
    <p data-testid="sync-last-line" class="text-muted-foreground">{{ lastLine() }}</p>

    <section v-if="paused" aria-label="Sync paused" data-testid="sync-problem">
      <p class="font-medium text-destructive">Sync paused</p>
      <p>{{ pausedMessage }}</p>
      <RouterLink
        to="/settings/bank"
        class="mt-2 inline-flex rounded-md border px-3 py-2"
        @click="$emit('navigate')"
      >
        Paste new token
      </RouterLink>
    </section>

    <ul
      v-if="!paused && problems.length > 0"
      aria-label="Sync problems"
      class="flex flex-col gap-3"
    >
      <li
        v-for="(problem, index) in problems"
        :key="index"
        data-testid="sync-problem"
        class="flex flex-col items-start gap-1 rounded-xl border border-destructive/40 p-3"
      >
        <span class="font-medium">{{ problem.account }}</span>
        <span class="font-medium text-destructive">{{ problem.state }}</span>
        <span>{{ problem.message }}</span>
        <BankFix v-if="problem.fix.kind === 'simplefin'" class="mt-1" />
        <RouterLink
          v-else
          :to="problem.fix.to"
          class="mt-1 inline-flex rounded-md border px-3 py-2"
          @click="$emit('navigate')"
        >
          {{ problem.fix.kind === 'suspect-rows' ? 'See suspect rows' : 'See account' }}
        </RouterLink>
      </li>
    </ul>

    <ul
      v-if="quiet.length > 0"
      aria-label="Quiet"
      class="flex flex-col gap-1 text-muted-foreground"
    >
      <li v-for="state in quiet" :key="state.accountId" data-testid="sync-quiet">
        {{ state.account }}: {{ state.message }}
      </li>
    </ul>

    <p v-if="!paused && problems.length === 0 && quiet.length === 0">All good.</p>

    <div v-if="!paused" class="flex flex-col items-start gap-1">
      <button
        type="button"
        class="rounded-md border px-3 py-2 disabled:opacity-50"
        :disabled="syncing || wait() !== null"
        @click="$emit('syncNow')"
      >
        {{ syncing ? 'Syncing…' : 'Sync now' }}
      </button>
      <p v-if="wait()" data-testid="sync-wait" class="text-muted-foreground">{{ wait() }}</p>
      <p data-testid="sync-requests" class="text-muted-foreground">
        {{ sync.requests_left }} of {{ sync.request_cap }} requests left today
      </p>
      <p v-if="failure" role="alert" class="text-destructive">{{ failure }}</p>
    </div>

    <RouterLink to="/settings/bank" class="self-start underline" @click="$emit('navigate')">
      Run history
    </RouterLink>
  </div>
</template>
