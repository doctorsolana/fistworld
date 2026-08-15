# Company Economy

Status: implemented foundation and permanent design reference, audited 2026-08-15. This document
defines the boundary between people, companies and operating sites; the civic
market rules remain in [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md).

## Agreed model

- A `PersonId` or later a `ClanId` owns whole ordinary shares in a stable `CompanyId`. Every company has exactly 1,000 issued shares; ownership percentages are display-only derivations.
- A company operates one or more productive sites identified by `BuildingId`.
- Houses and household consumption remain personal/household property; they are not company assets.
- Company cash pays payroll, external inputs, municipal delivery fees, taxes and other obligations for every operated site, including sites in different settlements.
- Workers retain exactly one job and one workplace. Their wages are paid by the company but attributed to that site's operating ledger.
- An eligible unemployed Company Master gets first refusal on one vacancy in their own company. This makes owner = Master = worker the normal one-site founding pattern without bypassing skill requirements or creating a second job. A wealthy Master may later hire a replacement and leave daily production when payroll is secure.
- Compatible sites belonging to the same company may exchange inputs directly. These goods never enter the Moot Hall inventory, never receive a public listing and never pay a market fee.
- Internal goods create equal bookkeeping credits and charges so site profitability remains visible. Consolidated company results eliminate both sides.
- A company is global for ownership, leadership, cash, obligations and consolidated accounting. Physical goods are local: every site and Moot listing belongs to one settlement and stock never teleports between towns.
- Each `(CompanyId, SettlementId)` branch owns one simple rule per good: an absolute number of units to retain locally and `Sell excess`/`Hold all`. The rule protects the branch total once, not once per building.
- Active company input requests have first claim on compatible owned output. The branch reserve comes next; only the uncommitted remainder may be collected for the public market.
- A Moot Steward can carry an internal shipment for a company at the civic rate of one penny per carried bulk unit. The Hall is the employer/dispatcher, not a waypoint or buyer.
- A private Storage Hall holds 2,400 bulk and opens up to four ordinary one-job Company Porter positions. Its porters move only their company's goods inside that settlement and charge no municipal delivery fee; their wages are the logistics cost.
- NPCs treat a Storage Hall as established branch infrastructure, not a founding trade: an autonomous applicant needs at least two other local company sites and cannot add a second depot to that branch. The tier-unlocked player permit remains freely purchasable.
- Workshops near 80% capacity can send bounded excess loads to an owned local Storage Hall. Processors can draw requested inputs back from it, and branch surplus stored there can still reach the public market.
- Cross-settlement transfers are deliberately not automatic. Later caravans and trade routes must name an origin, destination, cargo and physical carrier.
- Input sourcing modes are `PreferOwned`, `CheapestAvailable` and `OwnedOnly`.
- Company and clan identities remain separate. Before a business permit is issued, an entrepreneur explicitly founds a company, contributes personal coin, receives all 1,000 shares and becomes its Company Master. The share model supports later co-ownership, inheritance and multiple companies.

## Accounting invariants

1. Coin is never created or destroyed by a company transfer.
2. Internal transfers move no cash. Their equal site credit/charge is eliminated from company profit.
3. A successful municipal delivery debits company cash and credits the settlement treasury by exactly the same fee. A private Company Porter charges no municipal fee.
4. Failed deliveries charge no fee. Collected goods are either delivered or returned; they never disappear.
5. Company payroll expense is incurred once, paid once and attributed to exactly one workplace.
   Payroll closes at dawn and belongs to the shift which just ended, so it appears in the
   completed-day ledger rather than being moved into the new day's P&L.
6. Shareholder-contributed capital is not revenue. Dividends are distributions, not
   operating expenses. The site ledger's legacy `owner_withdrawals` field is the attributed
   distribution memo; spendable cash still moves only from `CompanyAccount`.
7. Taxes are assessed against real company operating profit, never internal bookkeeping turnover.
8. A site's reported result and the consolidated company result are explainable from retained bounded history.

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
3. **Dividend:** the Company Master may distribute drawable retained profit.
   The payment is divided pro rata over the current 1,000-share cap table. It is
   not a wage or operating cost.
4. **Share sale:** a buyer pays the selling shareholder and receives already
   issued shares. This moves no money into or out of the company unless a later
   primary-issuance mechanic is deliberately introduced.

Customers pay the company, suppliers and workers are paid by the company, and
taxes and delivery fees are paid by the company. The Master cannot casually
move company cash into their wallet; dividends are the explicit audited path.

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
firm deliberately enter Growth while a company with arrears or distressed sites
moves to Cautious and stops compounding its problems. If one NPC happens to master
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

The shared Moot inventory is a sales floor rather than free warehousing. A branch
may consign each good only until that good's public target is full, counting stock
already in a porter's cart. Excess stays at its producing site or Storage Hall and
becomes eligible as households clear earlier listings. This keeps a Wheat glut from
using every hall slot while food buyers wait for Bread, Fish or Flour.

The company pays only the actual permit fee. Suggested opening capital for a
processor—one input batch plus prudent opening payroll—is a decision/UI
recommendation, not escrow, and remains usable company cash. Surrendering an
unused permit returns its paid fee to the exact company. A sole owner may make
an explicit capital contribution; a co-owned company waits until retained cash,
a shareholder loan or a future approved share issue can fund it.

Autonomous firms open one position first and expose more of the building only
after production or revenue proves the operation; cash-tight and distressed
sites contract to one position. The processor's recommended reserve covers one complete recipe
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

## Runtime design

- Stable identity: `CompanyId`, `Company`, `CompanyShare`, `OperatedBy`.
- Finance: `CompanyAccount`, company liabilities and current/completed
  `CompanyDayLedger` snapshots. The open day's wage line may legitimately remain zero until
  dawn; completed payroll must remain visible in `previous_day` after consolidation.
- Site accounting: `BusinessAccount` and its bounded ledgers; per-building history remains the place for production, local costs and settings, never a second wallet.
- Supply policy: every enabled processor input has a player-facing stock-coverage target of 0–7 days, a sourcing mode and an optional preferred supplier `BuildingId`. The server converts days into bounded unit targets from the recipe, current staffing and storage capacity; its lower reorder threshold is internal hysteresis rather than another owner setting.
- Branch stock policy: `CompanyBranchPolicies` stores absolute retain units plus the public-sale toggle per local good. This is separate from processor input cover and uses stable `SettlementId`, so identical company operations in two towns cannot leak policy or inventory into each other.
- Staffing policy: `BusinessStaffingPolicy` is the operator's enabled-position target from zero to the building's architectural maximum. Closing a porter position waits for any active shipment and carried stock to reach a safe boundary before releasing the worker.
- Matching: build settlement/company/good indexes once per bounded economy pass; do not perform an all-company scan for every NPC.
- Delivery: persistent internal orders use stable building identities. An embodied civic or private porter receives a server-only routine only while performing one physical trip.
- Strategic simulation: collapse the identical order, goods, fee and ledger transaction without spawning a body.
- Decisions: exact own books plus public market observations, smoothed over several days. Daily operational reviews and staggered weekly capital reviews use deterministic `CompanyId` offsets.

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
- Company sites link into their settlement/building records; shareholders link into People. Each site exposes separate `View Details` and `Manage Site` actions. Both drill-down paths provide `Back to Company`, and repeated management clicks preserve scroll position. `Company Controls` opens the existing authoritative management surface instead of duplicating mutation controls in a read-oriented directory.
- **Full Ledger** pulls up to 365 completed days on demand by stable `CompanyId`. It consolidates sites across every settlement, eliminates internal supplier credits/buyer charges from profit, and retains those equal amounts as an audit memo. Charts cover P&L, cash/debt/assets, dividends/capital spending and internal flow; tables retain per-site contribution and recent daily records.

## NPC company decisions

- Survival order: obligations, payroll reserve, viable supply, turnaround, closure,
  investment, then shareholder distributions.
- A small founder normally works at their own first site. Company leadership is not itself a second paid job, and successful Masters can step back only after an available replacement and protected payroll make that choice credible.
- A person still holds exactly one active job. An off-shift employee who accepts a
  private construction permit resigns first and relinquishes every old workplace
  routine; someone in the middle of a shift is ineligible until that work is clear.
  Civic construction remains part of the Reeve's single civic appointment.
- New sites receive a probation period. The executable lifecycle is `New`, `Operating`,
  `Cash tight`, `Distressed`, `Insolvent`, `Liquidating`, `For sale` and `Closed`.
  Formal reduced-activity and mothball states remain future branch-lifecycle work.
- Investment uses expected throughput, contribution margin, labour, inputs, logistics, permit/capital cost and remaining payroll runway.
- Daily automatic pricing uses the previous day's observed wage cost per output unit
  plus the current replacement quote for exactly one unit's recipe inputs. Actual bulk
  procurement remains a real ledger expense, but is not mistaken for every unit's cost
  on the purchase day. Owners raise prices while scarce and selling through, and make
  strategy-bounded markdowns when stock grows much faster than sales. These are private
  decisions rather than municipal price controls.
- Company Master attributes influence automatic strategic posture. Once a company exists,
  its reviewed posture controls permit investment; founder attributes and deterministic
  bias still distinguish first-time entrepreneurs and otherwise comparable opportunities.
  Richer forecast-error, patience and management-capacity modelling remains future work;
  no controller may see private competitor books.
- Bounded executive history records Company Master, old/new strategy and a concrete reason.
  Permit-market diagnostics retain their separate opportunity scores; a full alternative-by-
  alternative executive score breakdown remains future work.

## Acceptance scenarios

- One owner, one business continues to behave like the present economy.
- Farm, Windmill and Bakery owned by one company privately move Wheat and Flour, and only Bread reaches the public market when configured that way.
- The same company operating in two settlements has one treasury but independent branch inventory, capacity and public-sale rules; no good crosses the boundary without a future physical trade route.
- A Storage Hall accepts local overflow, its Company Porters supply local owned processors without a civic delivery fee, and closing a porter position cannot lose an in-flight load.
- A different owner's processor cannot consume privately committed stock.
- `PreferOwned` falls back to the public market; `OwnedOnly` does not.
- Internal transfers remain possible with several suppliers/receivers without duplicate reservations.
- Municipal fees appear as receiving-site cost, company cost and civic income.
- Individual site internal revenue/input charges cancel exactly in company consolidation.
- Worker death/vacancy, owner death, share transfer, site sale, company insolvency and route failure preserve goods, coin, jobs and claims.
- Tactical 1x/10x/25x and strategic simulation agree on economic outcomes within physical timing tolerances.
- Long-run multi-seed scenarios show both entry and exit, no repeated processor spam, no permanently stranded edible stock and no decision/request leaks.
- The 1,000-NPC test remains within the existing server performance envelope.

## Implementation checklist

- [x] Stable company/share components and registries
- [x] Existing-world/company migration
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
- [x] Per-site enabled-position controls with graceful worker release
- [x] Pull-based cross-settlement company/site history archives and lab reports
- [x] Company directory, multi-company player portfolio, share market and supply-chain UI
- [x] NPC operational, dividend and capital controller
- [ ] Branch review/mothball/sale lifecycle
- [x] Unit and coin/share conservation tests
- [ ] Multi-seed economic tuning and explicit mothball acceptance tests
