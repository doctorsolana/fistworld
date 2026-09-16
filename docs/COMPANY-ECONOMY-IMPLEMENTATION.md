# Company Economy

Status: implemented foundation and permanent design reference; merchant, storage and
market-target status reconciled 2026-09-09. This document
defines the boundary between people, companies and operating sites; the civic
market rules remain in [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md).

## Agreed model

- A `PersonId` or later a `ClanId` owns whole ordinary shares in a stable `CompanyId`. Every company has exactly 1,000 issued shares; ownership percentages are display-only derivations.
- A company operates one or more productive sites identified by `BuildingId`.
- Houses and household consumption remain personal/household property; they are not company assets.
- Company cash pays payroll, external inputs, municipal delivery fees, taxes and other obligations for every operated site, including sites in different settlements.
- Workers retain exactly one job and one workplace. Their wages are paid by the company but attributed to that site's operating ledger.
- An eligible unemployed Company Master gets first refusal on one vacancy in their own company. This makes owner = Master = worker the normal one-site founding pattern without bypassing skill requirements or creating a second job. A financially secure Master may delegate daily production when a replacement and payroll are ready, but returns when an advertised position cannot otherwise be filled.
- Compatible sites belonging to the same company may exchange inputs directly. These goods never enter the Moot Hall inventory, never receive a public listing and never pay a market fee.
- Internal goods create equal bookkeeping credits and charges so site profitability remains visible. Consolidated company results eliminate both sides.
- A company is global for ownership, leadership, cash, obligations and consolidated accounting. Physical goods are local: every site and Moot listing belongs to one settlement and stock never teleports between towns.
- Each `(CompanyId, SettlementId)` branch owns one simple rule per good: an absolute number of units to retain locally and `Sell excess`/`Hold all`. The rule protects the branch total once, not once per building.
- Active company input requests have first claim on compatible owned output. The branch reserve comes next; only the uncommitted remainder may be collected for the public market.
- A Moot Steward can carry an internal shipment for a company at the civic rate of one penny per carried bulk unit. The Hall is the employer/dispatcher, not a waypoint or buyer.
- A private Storage Hall holds 2,400 bulk and opens up to four ordinary one-job Company Porter positions. Its porters move only their company's goods inside that settlement and charge no municipal delivery fee; their wages are the logistics cost.
- NPCs normally treat a Storage Hall as established branch infrastructure: an autonomous applicant needs at least two other local company sites and cannot add a second depot to that branch. Funded civic export or merchant opportunity signals can also support a young standalone logistics company without that two-site prerequisite. Ordinary opportunity/funding checks still apply; the tier-unlocked player permit remains freely purchasable.
- Workshops near 80% capacity can send bounded excess loads to an owned local Storage Hall. Processors can draw requested inputs back from it, and branch surplus stored there can still reach the public market.
- Cross-settlement transfers are explicit company assets. Buyer-funded civic contracts use locked pickup/delivery stops; player-authored merchant routes use two to eight ordered town stops with `Buy`, `Load owned stock`, `Sell` or `Unload to storage` instructions. Every route names one home Storage Hall, one good, a finite cart target and an employed Company Porter. No company is tagged as a special trade-company type.
- Input sourcing modes are `PreferOwned`, `CheapestAvailable` and `OwnedOnly`.
- Company and clan identities remain separate. Before a business permit is issued, an entrepreneur explicitly founds a company, contributes personal coin, receives all 1,000 shares and becomes its Company Master. The share model supports later co-ownership, inheritance and multiple companies.

## Accounting invariants

1. Coin is never created or destroyed by a company transfer.
2. Internal transfers move no cash. Their equal site credit/charge is eliminated from company profit.
3. A successful local municipal delivery debits company cash and credits the settlement treasury by exactly the same fee. A local private Company Porter charges no municipal fee. An inter-settlement contract instead escrows buyer cash, pays the source seller at collection and credits the carrier company's warehouse ledger only when cargo reaches the destination.
4. Failed deliveries charge no fee. Collected goods are either delivered or returned; they never disappear.
5. Company payroll expense is incurred once, paid once and attributed to exactly one workplace.
   Payroll closes at dawn and belongs to the shift which just ended, so it appears in the
   completed-day ledger rather than being moved into the new day's P&L.
6. Shareholder-contributed capital is not revenue. Dividends are distributions, not
   operating expenses. The site ledger's legacy `owner_withdrawals` field is the attributed
   distribution memo; spendable cash still moves only from `CompanyAccount`.
7. Taxes are assessed against real company operating profit, never internal bookkeeping turnover.
8. A site's reported result and the consolidated company result are explainable from retained bounded history.
9. A merchant `Buy` stop debits real company cash and pays the exact public sellers. A `Sell` stop creates a seller-owned consignment; asking value is not revenue and the company is paid only when a real later buyer clears it. `Load` and `Unload` move owned stock without cash or public listings.

## How money enters and leaves a company

The company is the legal and accounting owner of operating money. A building
does not have an independent purse or cash allocation. `CompanyAccount.cash`
is the one authoritative treasury used by every site. A site's
`BusinessAccount` is only a cost-centre ledger: it attributes revenue, wages,
inputs, fees, taxes, liabilities, production and profit to that workplace.
Company cash remains ordinary spendable treasury money during construction.
Only a purchased permit's actual fee is briefly refundable; materials, inputs,
wages and expansion are paid from the same company cash when each cost occurs.

There are only four person/company cash boundaries:

1. **Capital contribution:** a sole founder may use personal coin to fund a new
   company or the unfunded part of a project. This increases company cash and
   `contributed_capital`; it is not sales revenue or profit. A co-owned company
   cannot silently accept an automatic Master top-up because that would enrich
   the other shareholders without compensation. It must use retained cash until
   explicit shareholder loans or primary share issuance are implemented.
2. **Wage:** the company pays a worker for one site's position. It is a company
   expense attributed to that site and ordinary personal income for the worker.
3. **Dividend:** the Company Master may distribute drawable retained profit, and
   may choose the amount (`DistributeDividend { pennies }`, `u64::MAX` = everything
   distributable). The finance pass clamps the request to what is distributable at
   that moment (consolidated retained profit, i.e. every site's revenue less every
   site's expenses and prior withdrawals summed before clamping at zero so a
   loss-making site offsets a profitable sibling, capped by treasury cash above every
   site's payroll, input, tax and operating reserves) and reports the clamped
   figure instead of refusing on a stale snapshot. `CompanyDividendCapacity` is
   published once per company per world day, after each manual request, and once
   on the tick after a company is founded mid-day (its default `day == u32::MAX`
   marks it never reviewed), so a new company's headroom is visible the same day. Manual payouts are bounded only
   by those reserves: they ignore `max_daily_dividend` and the automatic
   one-per-day cadence and never touch `last_dividend_day`. The payment is divided
   pro rata over the current 1,000-share cap table with the shared
   `pro_rata_split` (wide integer floor; only whole-penny rounding goes to the
   stable first shareholder), so a client preview and the server payout agree
   penny for penny. It is not a wage or operating cost. Every receiving wallet is
   checked before any treasury debit. If one cannot accept its payment, the whole
   distribution and its profit entitlement remain with the company. The site memo
   (`owner_withdrawals`) is attributed against each site's own retained profit in
   stable building order, so a site leaving the company takes its revenue and its
   paid-out share together and already-distributed profit never becomes
   distributable again.
4. **Share sale:** a buyer pays the selling shareholder and receives already
   issued shares. This moves no money into or out of the company unless a later
   primary-issuance mechanic is deliberately introduced.

Customers pay the company, suppliers and workers are paid by the company, and
taxes and delivery fees are paid by the company. The Master cannot casually
move company cash into their wallet; dividends are the explicit audited path.

## Work, ownership and basic leisure

Employment and share ownership are separate relationships. Anyone filling a
real position—including a founder, Company Master or minority shareholder—is
owed the workplace's ordinary wage. That wage is recorded before profit and may
become arrears if company cash is short. Ownership pays only the pro-rata
dividend, so future co-owners cannot receive free labour or a disguised unequal
distribution.

NPC Masters do not retire at one universal coin threshold. Once per day they
compare the offered wage with a personal reservation wage. The reservation
wage rises gradually with the number of days their liquid wallet can cover at
the town's current ready-meal cost, has stable person-to-person variation, and
rises modestly when the Master operates several sites. A Master delegates only
when another resident is looking for work and the company can protect the
replacement's payroll. This gives successful owners more visible free time
without making wealth an on/off state.

`Chilling` is intentional non-participation in the general labour market, not
permanent retirement. If one of the Master's sites advertises a productive
position and no other resident can take it, the Master seeks work again and
receives first refusal through the ordinary vacancy matcher. If the wage later
becomes attractive relative to their security, they may also return by choice.
Observed resting Masters use the bounded ambient-life system and prefer cached
spots around a local Market when one exists. Private Tavern meal service and compact
daily plans already attach real purchases to civilian routines. Clothing, house
maintenance and other future trades can extend that boundary; owner-funded house
extensions are already implemented separately.

## Three management strategies

The player and NPC Company Masters use the same three choices. These are risk
postures; `Mothballed`, `Liquidating` and other operating states are separate.

| Strategy | Ordinary margin | Payroll reserve | Input cover | Normal daily price step |
| --- | --- | --- | --- | --- |
| Aggressive | 7.5% | 2 days | 2 days | 7% |
| Balanced | 15% | 3 days | 2 days | 5% |
| Conservative | 20% | 5 days | 4 days | 3% |

These are authored starting preferences, not fixed prices, wages or daily output
quotas. Actual sales, funded demand, stock, inputs, debts and available cash drive
reviews. Staff work their scheduled shifts while real inputs and storage permit.
A company with arrears becomes Conservative; financially healthy expansion can
justify Aggressive. Explicit player strategy remains authoritative until company
autopilot is re-enabled, and reaches every automatic sibling site on the same day
without waiting for the next executive review. Sites with their own autopilot
disabled retain their manual overrides. A temporarily mothballed business still
allows policy edits; an explicit positive staffing target requests its reopening. Liquidation
and completed closure cannot be undone by changing a wage or strategy button.

## Formation, acting company and funding a new site

A person must found a legal company before buying or receiving a business
permit. Formation is one atomic operation: personal coin is debited, the new
treasury is credited, all 1,000 shares are issued to the founder and the founder
becomes Company Master. A hero who masters several companies selects an explicit
`ACTING AS` CompanyId at the Hall. Every quote, purchase, permit, worksite and
completed business retains that exact identity; the server never guesses the
oldest company and never silently falls back to a personal wallet.

An established NPC company considers its prudent cash position before opening a
site. Its most recently reviewed company strategy—not the founder's original
temperament—is authoritative for subsequent investment. This lets a profitable
firm deliberately become Aggressive while a company with arrears or distressed sites
becomes Conservative and stops compounding its problems. If one NPC happens to master
several firms, autonomous planning selects the oldest stable `CompanyId`
deterministically; player-issued permits still require the explicit `ACTING AS`
selection described above. Before the planner calls retained money "available",
every site protects:

- a first-time or sole-owner NPC contributor's final three personal coins, so
  founding a company cannot knowingly spend all immediate food liquidity;
- unpaid wage claims;
- unpaid tax claims;
- the company's configured number of enabled-position payroll days.

Processor input protection is a cash reserve for the shortfall between its
configured target and the input already physically held at that site; owned
stock is not repeatedly budgeted as though it still had to be purchased. The
company also protects one ordinary two-coin operating buffer across its pooled
treasury, not one duplicate buffer per cost centre. This distinction is
load-bearing for circulation: a vertically integrated firm may retain prudent
working capital without swallowing nearly every coin in a small settlement.

The shared Moot inventory is a sales floor with separate capacity for each good.
Public target stock guides planning and pricing; it does not cap consignments.
Owned-input reservations and branch retain/sell policy still protect private stock,
and the physical per-good market compartment bounds delivery. A Wheat glut cannot
consume the slots reserved for Bread, Fish or Flour. See [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md).

The company pays only the actual permit fee. Suggested opening capital for a
processor—one input batch plus prudent opening payroll—is a decision/UI
recommendation, not escrow, and remains usable company cash. Surrendering an
unused permit returns its paid fee to the exact company. A sole owner may make
an explicit capital contribution; a co-owned company waits until retained cash,
a shareholder loan or a future approved share issue can fund it.

Autonomous firms open one position first. After that, a cached daily operating
plan tests every additional position against the output that can plausibly sell,
the site's own stock and listings, current recipe/input quotes, market fees and
the offered wage. Hiring and release move by at most one position per day; a
cash-tight or distressed site cannot expose more than one. This is a staffing
forecast, not an output quota: employed workers continue through their full shift
while physical resources, purchased inputs and storage permit, and input
procurement is not capped by the forecast. Public input purchases retain one
planned payroll day after existing liabilities, including the first purchase;
an unproven processor cannot spend that wage on a multi-day input stockpile.
The processor's recommended reserve covers one complete recipe
batch plus that prudent opening payroll. If the input market is temporarily
unquoted, the applicant budgets against 260% of the input's base value so an
otherwise viable Bakery does not spend all of its first cash on idle wages
before it can buy two Flour. This assumption changes entry funding only—it is
not a price ceiling or a guaranteed trade.

The permit fee and construction value are capital expenditure and increase the
site/company asset book value. They are shown separately from operating P&L, so
buying a Windmill does not make an otherwise profitable milling day appear to
have suffered an operating loss.

Buying an existing failed business still uses the existing personal,
recapitalised takeover flow. Before companies enter that market, its listing
must separate two amounts that the old mechanic currently calls one price:

- **purchase consideration** leaves company cash, is paid to the living seller
  or insolvent estate/creditors, and increases the acquiring company's asset
  book value without entering operating P&L;
- **opening working capital** remains in the single company treasury and must
  not be counted as either revenue or acquisition expense.

Keeping those legs separate prevents a company from repeatedly "buying" failed
sites with the same internally moved cash. Company-funded property acquisitions
and formal asset depreciation are future extensions of this boundary, not
hidden operating expenses.

## Death and legal continuity

A co-owned company does not disappear because its Master or one shareholder
dies. Until wills and estate auctions exist, the deceased holding passes to the
largest living co-owner (stable `PersonId` resolves a tie), that person becomes
Company Master, impossible share offers are reconciled, and the company keeps
its sites and obligations. This is an intentionally conservative survivorship
rule, not the final inheritance design.

A sole proprietor has no surviving shareholder. Their sites retain the old
CompanyId while stock, company-funded permit refunds and creditor claims settle,
then enter the ordinary property market. Every completed site and active project
records a sparse, stable branch settlement even while it uses default stock rules.
Once the last site and permit leave an ownerless company, any unclaimed residual
treasury escheats to that last known settlement before the legal shell is retired;
coin can no longer remain trapped in a dead, site-less firm. A living shareholder
may deliberately retain a funded empty company for a later investment. If an
ownerless disconnected record has no known branch, the server preserves it rather
than destroying money or guessing a recipient.

Private wage creditors are retained by `PersonId` in the server-only
`PrivatePayrollClaims` ledger; `BusinessAccount.wage_arrears` remains the one
replicated aggregate liability. Ordinary job changes never erase these claims or
give them to replacement staff. Scarce payroll is apportioned by debt, with daily
rotation for indivisible pennies. Liquidation takes the existing named claims,
and an early takeover returns them to the operating ledger. Recipient availability
is checked before money and debt are reduced.

When a worker dies, the mortality pass settles the affordable portion of that
person's claims into the estate before distributing it to the household or Hall.
The remaining unpaid amount is explicitly defaulted under the current mortality
policy; other workers' debts remain owed. This does not implement family inheritance
or long-lived estate creditors. The claim scan occurs only when there are deaths.

## Runtime design

- Stable identity: `CompanyId`, `Company`, `CompanyShare`, `OperatedBy`.
- Finance: `CompanyAccount`, company liabilities and current/completed
  `CompanyDayLedger` snapshots. The open day's wage line may legitimately remain zero until
  dawn; completed payroll must remain visible in `previous_day` after consolidation.
- Dividend capacity: `CompanyDividendCapacity` (`day`, `distributable`, `protected_reserves`,
  `retained_profit`, `last_paid_day`, `last_paid`; `u32::MAX` = never) is published by
  `review_company_finance` after its payouts, once per company per world day and again for
  the requested company after each manual request, through an off-component copy and
  `set_if_neq`, so an unchanged figure never advances the replication tick and nothing is
  written per tick. It is a display/drafting snapshot; the pass re-derives the live figure
  when it pays.
- Site accounting: `BusinessAccount` and its bounded ledgers; per-building history remains the place for production, local costs and settings, never a second wallet.
- Supply policy: every enabled processor input has a player-facing stock-coverage target of 0–7 days, a sourcing mode and an optional preferred supplier `BuildingId`. The server converts days into bounded unit targets from the recipe, current staffing and storage capacity; its lower reorder threshold is internal hysteresis rather than another owner setting.
- Branch stock policy: `CompanyBranchPolicies` stores absolute retain units plus the public-sale toggle per local good. This is separate from processor input cover and uses stable `SettlementId`, so identical company operations in two towns cannot leak policy or inventory into each other.
- Staffing policy: `BusinessStaffingPolicy` is the operator's enabled-position target from zero to the building's architectural maximum. Closing a position requests a graceful release: active production deposits its last load, service workers leave through their door, and carriers settle their shipment before employment is removed. Reopening the position before that handoff cancels the release request.
- Matching: build settlement/company/good indexes once per bounded economy pass; do not perform an all-company scan for every NPC.
- Delivery: persistent internal orders use stable building identities. An embodied civic or private porter receives a server-only routine only while performing one physical trip. If neither service covers a workplace, one of its employees may interrupt production and carry only a personal-capacity load to or from the public market; that employee cannot act as a general porter for another firm.
- Observation-independent delivery: the same worker physically loads, carries and unloads goods everywhere. Employee self-haul pauses production for the actual trip; specialist cart delivery leaves the producer available to work. No aggregate transaction or estimated labour deduction replaces that lifecycle.
- Decisions: exact own books plus public market observations, smoothed over several days. Daily operational reviews and staggered weekly capital reviews use deterministic `CompanyId` offsets.

Production first deposits at its own workplace. If its final load cannot fit,
there is no applicable porter and the sale/reserve policy allows consignment,
an employee safely outside may carry that existing output directly to the public
counter. The production routine remains attached while freight owns movement,
keeping other outputs and the final return obligation intact. Goods retain
company title; partial deliveries decrement only the units actually moved,
including failed-sale returns and purchased inputs. Closing a producer's position
waits for its output and active commitments, but unrelated personal materials
are preserved without blocking release. These contracts are implemented in
`village/commerce.rs`, `employment.rs` and `worker_activity/`.

## Player-facing controls

- The encyclopedia's **Companies** tab is the world directory and player portfolio. It supports `All firms`, `My holdings` and `Shares for sale` views, keeps companies with several sites or several shareholders legible, and never conflates Hero-wallet cash with company cash.
- The portfolio header shows the Hero wallet, number of direct company holdings, number of Company Master offices and a pro-rata accounting-interest estimate. That estimate is company cash plus asset book value less wage/tax debt; it is explicitly not a quoted market value or spendable personal coin.
- Company overview: stable identity, status, exact 1,000-share cap table, public offers, Company Master, every operated site, cash, obligations, external revenue, all real costs, consolidated profit, capital expenditure, book value, dividends, policy and bounded executive decisions.
- Site overview: output, finite capacity, workers, enabled/open positions, attributed wages, internal/external flows, logistics, site result and current supply commitments.
- Input rule: days of stock cover, source priority (`Company first`, `Best value` or `Company only`) and maximum landed cost. The panel visualises current stock against the derived target; players never have to coordinate separate reorder and target numbers.
- Local-goods rule: the company page groups sites by settlement and shows their combined held bulk, finite capacity and Storage Halls. Every good has an absolute retain-unit control and one `Sell excess`/`Hold all` toggle. A one-town company sees one plainly named local section; a multi-town company sees a separate section for each town.
- Site management keeps input-cover selectors for processors and adds an open-position selector from zero to the site's physical maximum. Changing a player-owned staffing or stock target pauses the relevant autopilot so it cannot silently overwrite the choice.
- Current controls expose sourcing preference, preferred supplier, stock, target, local
  capacity and whether civic or private logistics is available. A replicated live-shipment
  inspector with carrier phase and route-failure detail remains future debugging UI.
- Caravan routes appear beside operating sites as first-class company assets. Their cards show
  the home warehouse, assigned caravaner, cargo, current stop, ordered timetable, repeat/manual
  service, completed trips, cash spent, freight earned and merchant asking-value consigned.
  A Company Master can create or edit an idle merchant route through the same company panel:
  choose two to eight ordered towns, cycle each town's instruction, move or remove stops, set
  cargo target, buy ceiling, sale floor and continuous/one-circuit service, then dispatch,
  mothball or reopen it. Contract routes use the same presentation but their civic stops stay locked.
  The final stop may return to the home town, so one circuit can buy elsewhere and finish by
  selling at the home market or unloading into the home Storage Hall.
  Creation, edits, reopening and dispatch reject schedules spanning different known
  founding land groups, including through unclassified intermediate stops. Automatic
  merchant opportunities use the same gate. These server-only tags prevent unsupported
  overland commitments; they do not replace normal navigation. Explicit maritime
  routes use their own port, crew and class-certified water path instead of this land gate.
- Company sites link into their settlement/building records; shareholder and observed worker names link to durable People records, with a return action. Each site exposes `View Details` and `Site Settings`. Both drill-down paths provide `Back to Company`, and repeated management clicks preserve scroll position. `Company Settings` opens by `CompanyId`, without choosing an arbitrary site or requiring one to be observed or operating.
- Company direction, executive autopilot, dividends, Master appointments and share offers use `HeroCompanyOrder`; site wages, positions, asking prices, procurement and local strategy use `HeroBusinessOrder`. The server resolves the actual company or site and authenticates the player again. Company replies include the company identity; site replies include their mapped site entity, so late results stay with their originating control context.
- Dividend controls live in COMPANY SETTINGS, TREASURY & DIVIDENDS (`client/src/ui/business_management`). The SHAREHOLDER DIVIDENDS row binds the replicated `CompanyDividendCapacity` as `Available now X coin (day d) · reserves R coin · last paid P coin on day d` (or `never paid` / `awaiting the first finance review`) above the policy line. The Company Master drafts an amount with `-1 COIN`, `+1 COIN`, `25%`, `50%` and `ALL` (a client-side `DividendDraft`, seeded at everything distributable and following the published headroom until the player steps it, so a company opened before its snapshot arrives does not stay pinned at zero; every step clamps to `0..=distributable`) and confirms with `DISTRIBUTE X COIN`, whose payload is `DistributeDividend { pennies: min(draft, distributable) }`. The confirm control always exists; with nothing distributable it reads `DISTRIBUTE 0.00 COIN` and sends `pennies: 0`, which the server refuses with a plain message, so the row never respawns when the headroom crosses zero. The IF DISTRIBUTED NOW row previews `X coin · X/100 coin per 10 shares · your S shares receive Z coin`, with `Z` taken from the shared `pro_rata_split` of the exact replicated cap table (holders without authority preview a full distribution). The deferred `HeroCompanyResult` lands one tick later in the panel's feedback line and in the Companies page's receipt note through the unchanged `receive_company_policy_results` path; nothing on the client assumes an immediate reply. The Companies encyclopedia page shows the same snapshot read-only as the DISTRIBUTABLE line under GOVERNANCE and, for shareholders, a Your Position note with the exact take of a full distribution; the amount picker exists only in the management panel.
- Choosing a company strategy pauses automatic executive strategy changes and reaches only sites still following company policy. A local strategy choice pauses that site's management and leaves company defaults and siblings untouched. Re-enabling site autopilot immediately adopts the current company default; re-enabling company autopilot resumes executive review without erasing manual site overrides. Separate manual wage and asking-price switches remain authoritative.
- Company finance/governance remains available when its only site is closed or absent. Physical branch-stock controls still require an owned site in the named settlement. Operating decisions require the appointed Master; appointing a Master requires more than 500 shares and a shareholder candidate. Shareholders can list only their own interest, and purchases settle the exact cap table against real wallets. Sole-owner capital contributions reject insufficient funds or receiving-ledger overflow before any debit. Dividends remain requests to the existing liability/reserve-aware finance pass, never an immediate site withdrawal: the order handler checks only authority and a positive amount, enqueues the request with the requester's `PersonId` and link, and sends no immediate reply. The finance pass pays or refuses on the next world tick and records a `DividendOutcome` on every path, including a request whose company entity vanished in between (`CompanyUnavailable`); `report_dividend_outcomes` (NetIngress, right after the order handler) then sends exactly one `HeroCompanyResult` per request naming the coin paid, the rate per 10 shares, the requester's own take for their share count and the reserves/not-yet-earned cash held back, or the concrete refusal (no operating site, unreachable shareholder, nothing distributable, full wallet, failed debit, company gone). A refused request never marks the replicated `CompanyAccount` changed: wallet capacity and the zero-amount case are validated before the treasury is borrowed mutably. Contributed capital is never distributable and is reported as "not yet earned".
- **Full Ledger** pulls up to 365 completed days on demand by stable `CompanyId`. It consolidates sites across every settlement, eliminates internal supplier credits/buyer charges from profit, and retains those equal amounts as an audit memo. Charts cover P&L, cash/debt/assets, dividends/capital spending and internal flow; tables retain per-site contribution and recent daily records.

## Company ships and the public market

Company Masters order hulls by company and completed public port identity. The home
town must contain an operating company Storage Hall. A Coaster carries 480 bulk and
requires 48 Wood, 8 Iron, 12 Wool and 180 seconds of building; a Cog carries 1,200 bulk
and requires 96 Wood, 20 Iron, 24 Wool and 360 seconds. Orders wait for real listed
materials and protected company funding; Iron production remains future work, so an
ordinary world without Iron cannot conjure a finished ship. Limits are eight owned or
pending hulls per company, 32 open orders globally and one funded hull per port.

Construction reserves a complete purchased basket at the Hall, pays finite physical
Hall-to-shore deliveries and on-site labour, then consumes the recipe only on a safe
launch. Occupied berths delay launch; they do not retain an already-paid builder.
Cancelling an unfunded order creates no asset or refund. Funded cancellation preserves
company-owned materials and earned claims while returning only unused escrow. Capital
costs move from the order ledger to the finished hull exactly once; this book value is
not spendable company cash.

Cancelled hull orders first finish any carried-material return. Remaining goods
at the Hall or its completed public port are reconsigned to that town market under
an existing company warehouse's title, without paying the company early. A full
store preserves the exact remainder and keeps cancellation pending; retries occur
every thirty world seconds. After recovery, historical asset basis moves from the
order to the warehouse exactly once. Historical capex stays recorded, so company
capital is neither erased nor counted twice. This is the existing historical-cost
book model, not a new inventory valuation or depreciation system.

Maritime routes use the existing ordered Buy/Sell timetable with a particular company
ship and real warehouse porter as crew. The port accesses its town's existing
`MootMarket` and shared stock, rather than making another exchange or demanding an
additional inland freight economy. Only the ship's actual cargo crosses settlements.
Operating sales, purchases and port/market charges enter the home warehouse cost centre,
so existing company consolidation, taxes, payroll and dividend reserves continue to
apply. Hull orders, crew admission and water navigation remain server-authoritative;
class-specific depth, mast clearance and single-berth occupancy gates still apply.
The captain collects company-funded provisions at a real completed Marketplace or
Hall counter before returning to the port and boarding. Restocking while moored
requires disembarkation and the same physical trip; assignment remains owned and
departure waits. The purchase uses current listings and stock, records seller
claims and charges the home warehouse's input ledger only at actual collection.
Cancelling cannot refund spent cash while retaining the food or erase collected
meals. This 2026-09-16 revision awaits fresh connected acceptance; earlier dated
port captures retain their original scope.
This first slice does not implement Iron production, naval combat or a whole harbour
traffic scheduler. Sources: `server/src/world/ports`, `server/src/world/shipping` and
`shared/src/components/shipping.rs`; live evidence is tracked separately.

## NPC company decisions

- Survival order: obligations, payroll reserve, viable supply, turnaround, closure,
  investment, then shareholder distributions.
- A small founder normally works at their own first site and receives the same wage as any employee. Company leadership is not itself a second paid job. A successful Master can delegate after their market-linked reservation wage exceeds the offer and an available replacement plus protected payroll make that choice credible; an unfilled company vacancy can call them back.
- A person still holds exactly one active job. An off-shift employee who accepts a
  private construction permit resigns first and relinquishes every old workplace
  routine; someone in the middle of a shift is ineligible until that work is clear.
  Civic construction remains part of the Reeve's single civic appointment.
- New sites have a bounded opening trial until their first output/sale or the third
  daily boundary after opening. It survives a `Cash tight` classification so a
  late-day opening is not mistaken for an unsuccessful full shift. The trial still
  requires positive expected shift contribution; it creates no stock, money or
  perpetual staffing entitlement. Insolvency and closure still stop recruitment.
  The executable lifecycle is `New`, `Operating`, `Cash tight`, `Distressed`,
  `Insolvent`, `Mothballed`, `Liquidating`, `For sale` and `Closed`.
  A solvent mature site gradually releases staff and mothballs after a completed
  no-sale observation window with no unavailable demand and no profitable position.
  Its stock remains saleable; profitable unmet demand reopens the same plant with
  one position before any duplicate construction is considered.
- Investment uses marginal expected throughput, contribution margin, labour, inputs,
  logistics, permit/capital cost and remaining payroll runway. Idle positions and
  mothballed/liquidating/for-sale capacity suppress another plant. A Storage Hall
  is justified by the value of bulk stranded beyond recent cart throughput and
  existing free depot space, rather than a business-count ratio.
- Daily automatic pricing uses the previous day's observed wage cost per output unit
  plus the current replacement quote for exactly one unit's recipe inputs. Actual bulk
  procurement remains a real ledger expense, but is not mistaken for every unit's cost
  on the purchase day. Owners raise prices while scarce and selling through, and make
  strategy-bounded markdowns when stock grows much faster than sales. These are private
  decisions rather than municipal price controls.
- Price competition uses actual stocked rival listings. A historical clearance
  remains a last-sale observation, but cannot cancel scarcity increases after
  that stock is gone. An empty autonomous site can quote a small restart batch
  from its real requested volume, recipe inputs, site capacity and one worker's
  payroll; it only makes that cost-based correction when recorded buyer budgets
  cover the offer. This prevents a six-unit order being priced as though a full
  factory's hypothetical output could pay the wage.
- Funded unfilled demand keeps up to twelve exact bid bands per good/day before
  overflow coarsens canonical power-of-two price buckets. Each bucket rounds down,
  so compression cannot overstate purchasing power and does not depend on claim
  arrival order. Withdrawal maps the original bid to its current bucket, preserving
  the other quantities. Storage is fixed; there is no per-person market order book.
  These observations expire normally
  and are neither escrow nor guaranteed purchases. Production/restart estimates
  evaluate quantity actually affordable at each proposed price. Household retries
  replace their own epoch-scoped claims instead of multiplying demand.
- Staffed suppliers share shortage forecasts with exact stable remainders, so
  the sum of their claims cannot multiply the observed order. If their rated
  capacity at affordable bids and actual assigned-worker count
  cannot cover the funded order, one additional idle supplier with the cheapest
  viable restart offer may respond; stable building identity breaks equal-price
  ties independently of entity iteration order. Unstaffed competitors do not
  fragment one order into five unprofitable pieces. Price review and staffing
  use the same bounded daily allocation; each firm still decides its own
  profitable output and payroll, and actual buyers choose physical listings.
  Empty advertised jobs and unaffordable fixed asks do not block entry. A
  profitable owner-set fixed price remains eligible for automatic staffing
  without allowing the manager to change that price.
- Business review skips its settlement/company/workforce index construction on
  ticks where all sites have already reviewed the current day. A newly created
  site still receives its first review that day; the guard does not require a
  per-person economic routine or a wall-clock timer.
- Company Master attributes influence automatic strategic posture. Once a company exists,
  its reviewed posture controls permit investment; founder attributes and deterministic
  bias still distinguish first-time entrepreneurs and otherwise comparable opportunities.
  Richer forecast-error, patience and management-capacity modelling remains future work;
  no controller may see private competitor books.
- Bounded executive history records Company Master, old/new strategy and a concrete reason.
  Permit-market diagnostics retain their separate opportunity scores; a full alternative-by-
  alternative executive score breakdown remains future work.

## Labour, entry and trade review (2026-09-15)

Daily wage offers respond to vacancies and competing employers, bounded by the
site's contribution and one consolidated company reserve. Two payroll days, inputs
and unpaid liabilities are protected before raises; separate sites cannot promise
the same spare cash. The raise budget uses the larger of enabled positions and the
actual roster, so a staffing reduction cannot immediately spend still-owed payroll.
Completed shifts retain their agreed wage. See [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md)
for public pay and safe worker transitions. Automatic offers require funded employers;
a published but insolvent vacancy must not drive wage competition.

Processor entry accepts affordable physical imported inputs and demonstrated
supply; a local upstream building is no longer mandatory. Forecasts use the local
published wage distribution. Existing failed plants get a three-day recovery
opportunity, then suppress a challenger only when their restart is economically
credible. Pending/new plants still prevent simultaneous duplicate investment.

NPC takeovers review daily using actual demand, input costs, payroll runway and
personal reserves. Asking prices decline with exposure; a zero-price unwanted
plant is not automatically a good investment. Exact startup cash is contributed
alongside acquisition, and simultaneous buyers cannot reuse one demand forecast.

Merchant observations carry bounded bid curves. Empty shelves do not invent a
125%-of-base selling price. Automatic routes review executable quantities, actual
porter wages, competing prices, cash and protected working capital. Unsold imports
block repeat purchases and receive gradual cost-aware markdowns; aged stock may
be liquidated. Explicit player route control disables automatic management, and
shared listings containing manual-route cargo are protected from automatic repricing.
Civic freight prices include distance and wage costs, with affordable rebidding and
expiry/refund of unaccepted orders; existing escrow and seller payment remain real.

The daily owners are `village/economy.rs`, `development_market/investment.rs`,
`mortality/takeovers.rs` and `trade_routes/{merchant_economics,civic_review}.rs`.
They reuse per-market/company snapshots rather than rescanning all listings for each
candidate. `civic_labor.rs` and `employment.rs` gate safe job-choice retries hourly,
with at most one completed personal review per day. `commerce/payroll_claims.rs`
owns named debt; these decisions add no per-person tactical path searches.

## Acceptance scenarios

- One owner, one business continues to behave like the present economy.
- Farm, Windmill and Bakery owned by one company privately move Wheat and Flour, and only Bread reaches the public market when configured that way.
- The same company operating in two settlements has one treasury but independent branch inventory, capacity and public-sale rules; goods cross the boundary only through an explicit physical trade route.
- A three-town merchant timetable follows its stop order exactly. Public `Buy`/`Sell` works in any known settlement; private `Load`/`Unload` is accepted only where that company owns a completed Storage Hall.
- A Storage Hall accepts local overflow, its Company Porters supply local owned processors without a civic delivery fee, and closing a porter position cannot lose an in-flight load.
- A workplace with no applicable porter can still buy inputs and consign output through bounded employee self-haul, pausing production for the actual trip regardless of observation.
- A different owner's processor cannot consume privately committed stock.
- `PreferOwned` falls back to the public market; `OwnedOnly` does not.
- Internal transfers remain possible with several suppliers/receivers without duplicate reservations.
- Municipal fees appear as receiving-site cost, company cost and civic income.
- Individual site internal revenue/input charges cancel exactly in company consolidation.
- Worker vacancy, owner death, share transfer, site sale, company insolvency and route failure preserve goods, coin and attributable claims. Worker death settles or explicitly defaults that worker's wages before distributing the estate.
- At 1×/10×/25×, the canonical physical routines preserve cargo ownership, accounting and unfinished work through observation changes. Matched timing and economic outcomes require the bounded comparisons described in [SIMULATION-PARITY.md](SIMULATION-PARITY.md).
- Long-run multi-seed scenarios show both entry and exit, no repeated processor spam, no permanently stranded edible stock and no decision/request leaks.
- The 1,000-NPC test remains within the existing server performance envelope.

## Implementation checklist

- [x] Stable company/share components and registries
- [x] Live component reconciliation into the company model (not a disk-save migration)
- [x] One authoritative company treasury and consolidated daily ledger
- [x] Company-funded payroll, inputs, taxes and dividends
- [x] Company-funded new-site permits and personal capital contributions
- [x] Capital expenditure/book value separated from operating P&L
- [x] Site ledger internal/external split
- [x] Private sourcing policy and matching
- [x] Tactical direct-delivery routine and recovery
- [x] Strategic direct-delivery equivalent
- [x] Public-surplus reservation ordering
- [x] Settlement-local company branches with absolute per-good retention and public-sale controls
- [x] Finite Storage Hall inventory, private Company Porter employment and tactical/strategic local logistics
- [x] Stable company-owned route assets and buyer-funded civic contracts
- [x] Exact remote seller pickup, physical carried cargo, destination delivery and freight ledger
- [x] Bounded route trip history and company/Hall inspection UI
- [x] Player-authored two-to-eight-stop merchant route editor and manual/continuous dispatch
- [x] Physical merchant Buy/Load/Unload/Sell execution with real company cash and consignments
- [x] Per-site enabled-position controls with graceful worker release
- [x] Pull-based cross-settlement company/site history archives and lab reports
- [x] Company directory, multi-company player portfolio, share market and supply-chain UI
- [x] NPC operational, dividend and capital controller
- [x] Demand-led staffing, productive-site mothball and automatic reopening lifecycle
- [ ] Voluntary branch sale/reallocation across settlements
- [x] Bounded autonomous merchant trials using delayed/imprecise reports, real company
      cash, confidence/risk checks and mothball/retry rules
- [x] Employed porter hand-cart art, load state and wheel animation
- [ ] Strategic caravan promotion, larger wagons and multi-wagon route scaling
- [x] Unit and coin/share conservation tests
- [ ] Multi-seed economic tuning
- [x] Explicit marginal staffing, demand budget and mothball/reopen acceptance tests
