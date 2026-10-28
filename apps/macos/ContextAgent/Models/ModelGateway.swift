import Foundation

// ModelGateway (docs/desktop-protocol.md "ModelGateway"): the wire shapes the
// `gateway/*` methods relay from the gateway, and the usage maths behind the
// Account page's heatmaps. Quotas are gateway units; dollars are
// `quota / quota_per_unit`.

// MARK: - Wire shapes

/// `gateway/account`, also carried by the approved device poll.
struct GatewayAccount: Codable, Equatable {
    var id: Int
    var username: String
    var displayName: String
    var email: String
    var group: String
    var quota: Int
    var usedQuota: Int
    var requestCount: Int
    var quotaPerUnit: Double
    var displayInCurrency: Bool
    var token: GatewayToken?
    /// Whether requests draw on the plan or the balance first; nil from gateways that predate plans.
    var billingPreference: String?
    /// Active plans; empty means pay as you go from the balance, nil that
    /// the gateway predates plans (the Plan card then stays hidden).
    var subscriptions: [GatewaySubscription]?
    /// Credit only Context's requests can spend; nil from gateways that predate it.
    var contextCredit: GatewayContextCredit?
    /// Whether this key's device is identified; nil from gateways that predate device ids.
    var device: GatewayDevice?
    /// The referral program's summary; nil from gateways that predate it.
    var referral: GatewayReferralSummary?

    enum CodingKeys: String, CodingKey {
        case id, username, email, group, quota, token, subscriptions, device, referral
        case contextCredit = "context_credit"
        case displayName = "display_name"
        case usedQuota = "used_quota"
        case requestCount = "request_count"
        case quotaPerUnit = "quota_per_unit"
        case displayInCurrency = "display_in_currency"
        case billingPreference = "billing_preference"
    }

    init(id: Int, username: String, displayName: String, email: String, group: String, quota: Int, usedQuota: Int, requestCount: Int, quotaPerUnit: Double = 500_000, displayInCurrency: Bool = true, token: GatewayToken? = nil, billingPreference: String? = nil, subscriptions: [GatewaySubscription]? = nil) {
        self.id = id
        self.username = username
        self.displayName = displayName
        self.email = email
        self.group = group
        self.quota = quota
        self.usedQuota = usedQuota
        self.requestCount = requestCount
        self.quotaPerUnit = quotaPerUnit
        self.displayInCurrency = displayInCurrency
        self.token = token
        self.billingPreference = billingPreference
        self.subscriptions = subscriptions
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(Int.self, forKey: .id) ?? 0
        username = try c.decodeIfPresent(String.self, forKey: .username) ?? ""
        displayName = try c.decodeIfPresent(String.self, forKey: .displayName) ?? ""
        email = try c.decodeIfPresent(String.self, forKey: .email) ?? ""
        group = try c.decodeIfPresent(String.self, forKey: .group) ?? ""
        quota = try c.decodeIfPresent(Int.self, forKey: .quota) ?? 0
        usedQuota = try c.decodeIfPresent(Int.self, forKey: .usedQuota) ?? 0
        requestCount = try c.decodeIfPresent(Int.self, forKey: .requestCount) ?? 0
        quotaPerUnit = try c.decodeIfPresent(Double.self, forKey: .quotaPerUnit) ?? 500_000
        displayInCurrency = try c.decodeIfPresent(Bool.self, forKey: .displayInCurrency) ?? true
        token = try c.decodeIfPresent(GatewayToken.self, forKey: .token)
        billingPreference = try c.decodeIfPresent(String.self, forKey: .billingPreference)
        subscriptions = try c.decodeIfPresent([GatewaySubscription].self, forKey: .subscriptions)
        contextCredit = try c.decodeIfPresent(GatewayContextCredit.self, forKey: .contextCredit)
        device = try c.decodeIfPresent(GatewayDevice.self, forKey: .device)
        referral = try c.decodeIfPresent(GatewayReferralSummary.self, forKey: .referral)
    }

    /// Cents of credit as the gateway shows quota: dollars, or raw units.
    func formatCents(_ cents: Int) -> String {
        displayInCurrency ? UsageFormat.usd(Double(cents) / 100) : UsageFormat.compact(Int((Double(cents) / 100 * quotaPerUnit).rounded()))
    }

    /// Display name, else the username.
    var name: String {
        let trimmed = displayName.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? username : trimmed
    }

    /// One letter for the avatar.
    var initial: String {
        String(name.trimmingCharacters(in: .whitespacesAndNewlines).prefix(1)).uppercased()
    }

    var balanceUSD: Double { Double(quota) / max(quotaPerUnit, 1) }
    var usedUSD: Double { Double(usedQuota) / max(quotaPerUnit, 1) }

    /// A quota amount as the gateway shows it: dollars, or raw units.
    func formatQuota(_ amount: Int) -> String {
        displayInCurrency ? UsageFormat.usd(Double(amount) / max(quotaPerUnit, 1)) : UsageFormat.compact(amount)
    }

    /// `billing_preference` in words.
    var billingPreferenceTitle: String? {
        switch billingPreference {
        case "subscription_first": "Plan first, then balance"
        case "wallet_first": "Balance first, then plan"
        case "subscription_only": "Plan only"
        case "wallet_only": "Balance only"
        default: nil
        }
    }
}

/// `context_credit` of `gateway/account`: quota only requests made with this
/// device's key can spend, before any plan or the balance. `offerQuota` is
/// the welcome credit, claimable once (`eligible`) after a card or crypto
/// top-up. Amounts are quota units.
struct GatewayContextCredit: Codable, Equatable {
    var balance: Int
    var granted: Int
    var claimed: Bool
    var eligible: Bool
    var offerQuota: Int
    /// The welcome credit is once per device too: this key's device has not
    /// identified itself yet (older gateways never set it).
    var deviceRequired: Bool
    /// Another account already claimed the welcome credit on this device.
    var deviceClaimed: Bool

    enum CodingKeys: String, CodingKey {
        case balance, granted, claimed, eligible
        case offerQuota = "offer_quota"
        case deviceRequired = "device_required"
        case deviceClaimed = "device_claimed"
    }

    init(balance: Int = 0, granted: Int = 0, claimed: Bool = false, eligible: Bool = false, offerQuota: Int = 0, deviceRequired: Bool = false, deviceClaimed: Bool = false) {
        self.balance = balance
        self.granted = granted
        self.claimed = claimed
        self.eligible = eligible
        self.offerQuota = offerQuota
        self.deviceRequired = deviceRequired
        self.deviceClaimed = deviceClaimed
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        balance = try c.decodeIfPresent(Int.self, forKey: .balance) ?? 0
        granted = try c.decodeIfPresent(Int.self, forKey: .granted) ?? 0
        claimed = try c.decodeIfPresent(Bool.self, forKey: .claimed) ?? false
        eligible = try c.decodeIfPresent(Bool.self, forKey: .eligible) ?? false
        offerQuota = try c.decodeIfPresent(Int.self, forKey: .offerQuota) ?? 0
        deviceRequired = try c.decodeIfPresent(Bool.self, forKey: .deviceRequired) ?? false
        deviceClaimed = try c.decodeIfPresent(Bool.self, forKey: .deviceClaimed) ?? false
    }
}

/// `device` of `gateway/account`: whether the gateway knows which machine
/// this key belongs to (`ctx serve` identifies it by itself).
struct GatewayDevice: Codable, Equatable {
    var identified: Bool
    var attested: Bool
    var platform: String

    init(identified: Bool = false, attested: Bool = false, platform: String = "") {
        self.identified = identified
        self.attested = attested
        self.platform = platform
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        identified = try c.decodeIfPresent(Bool.self, forKey: .identified) ?? false
        attested = try c.decodeIfPresent(Bool.self, forKey: .attested) ?? false
        platform = try c.decodeIfPresent(String.self, forKey: .platform) ?? ""
    }
}

/// Who referred this account and what their gift is (`invited_by` of the
/// referral summary). Amounts are cents of credit.
struct GatewayInvitedBy: Codable, Equatable {
    var displayName: String
    var giftCents: Int
    var giftUnlockedCents: Int
    var status: String

    enum CodingKeys: String, CodingKey {
        case status
        case displayName = "display_name"
        case giftCents = "gift_cents"
        case giftUnlockedCents = "gift_unlocked_cents"
    }

    init(displayName: String = "", giftCents: Int = 0, giftUnlockedCents: Int = 0, status: String = "") {
        self.displayName = displayName
        self.giftCents = giftCents
        self.giftUnlockedCents = giftUnlockedCents
        self.status = status
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        displayName = try c.decodeIfPresent(String.self, forKey: .displayName) ?? ""
        giftCents = try c.decodeIfPresent(Int.self, forKey: .giftCents) ?? 0
        giftUnlockedCents = try c.decodeIfPresent(Int.self, forKey: .giftUnlockedCents) ?? 0
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? ""
    }
}

/// `referral` of `gateway/account`, and the top of `gateway/referral`: my
/// code and link, my split of the match pool, the terms, totals. Amounts
/// are cents of credit; `shareRatePercent` is a percentage.
struct GatewayReferralSummary: Codable, Equatable {
    var enabled: Bool
    var code: String
    var link: String
    var poolCents: Int
    var splitRefereeCents: Int
    var minSplitRefereeCents: Int
    var shareRatePercent: Int
    var shareCapCents: Int
    var shareDays: Int
    var earnedCents: Int
    var refereeCount: Int
    var attachAllowed: Bool
    var attachWindowEndsAt: Int64
    var invitedBy: GatewayInvitedBy?

    enum CodingKeys: String, CodingKey {
        case enabled, code, link
        case poolCents = "pool_cents"
        case splitRefereeCents = "split_referee_cents"
        case minSplitRefereeCents = "min_split_referee_cents"
        case shareRatePercent = "share_rate_percent"
        case shareCapCents = "share_cap_cents"
        case shareDays = "share_days"
        case earnedCents = "earned_cents"
        case refereeCount = "referee_count"
        case attachAllowed = "attach_allowed"
        case attachWindowEndsAt = "attach_window_ends_at"
        case invitedBy = "invited_by"
    }

    init(enabled: Bool = false, code: String = "", link: String = "", poolCents: Int = 0, splitRefereeCents: Int = 0, minSplitRefereeCents: Int = 0, shareRatePercent: Int = 0, shareCapCents: Int = 0, shareDays: Int = 0, earnedCents: Int = 0, refereeCount: Int = 0, attachAllowed: Bool = false, attachWindowEndsAt: Int64 = 0, invitedBy: GatewayInvitedBy? = nil) {
        self.enabled = enabled
        self.code = code
        self.link = link
        self.poolCents = poolCents
        self.splitRefereeCents = splitRefereeCents
        self.minSplitRefereeCents = minSplitRefereeCents
        self.shareRatePercent = shareRatePercent
        self.shareCapCents = shareCapCents
        self.shareDays = shareDays
        self.earnedCents = earnedCents
        self.refereeCount = refereeCount
        self.attachAllowed = attachAllowed
        self.attachWindowEndsAt = attachWindowEndsAt
        self.invitedBy = invitedBy
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        code = try c.decodeIfPresent(String.self, forKey: .code) ?? ""
        link = try c.decodeIfPresent(String.self, forKey: .link) ?? ""
        poolCents = try c.decodeIfPresent(Int.self, forKey: .poolCents) ?? 0
        splitRefereeCents = try c.decodeIfPresent(Int.self, forKey: .splitRefereeCents) ?? 0
        minSplitRefereeCents = try c.decodeIfPresent(Int.self, forKey: .minSplitRefereeCents) ?? (poolCents + 1) / 2
        shareRatePercent = try c.decodeIfPresent(Int.self, forKey: .shareRatePercent) ?? 0
        shareCapCents = try c.decodeIfPresent(Int.self, forKey: .shareCapCents) ?? 0
        shareDays = try c.decodeIfPresent(Int.self, forKey: .shareDays) ?? 0
        earnedCents = try c.decodeIfPresent(Int.self, forKey: .earnedCents) ?? 0
        refereeCount = try c.decodeIfPresent(Int.self, forKey: .refereeCount) ?? 0
        attachAllowed = try c.decodeIfPresent(Bool.self, forKey: .attachAllowed) ?? false
        attachWindowEndsAt = try c.decodeIfPresent(Int64.self, forKey: .attachWindowEndsAt) ?? 0
        invitedBy = try c.decodeIfPresent(GatewayInvitedBy.self, forKey: .invitedBy)
    }

    /// The referrer's own share of the pool.
    var splitReferrerCents: Int { max(0, poolCents - splitRefereeCents) }
}

/// One referee of `gateway/referral`: their stage and what they earned me.
struct GatewayReferee: Codable, Equatable, Identifiable {
    var id: Int
    var displayName: String
    /// `signed_up` (no payment yet) · `active` · `completed` · `denied` · `frozen`.
    var stage: String
    var status: String
    var denyReason: String
    var poolCents: Int
    var poolUnlockedCents: Int
    var poolPaidReferrerCents: Int
    var poolPaidRefereeCents: Int
    var sharePaidCents: Int
    var shareCapCents: Int
    var earnedCents: Int
    var daysLeft: Int
    var createdAt: Int64

    enum CodingKeys: String, CodingKey {
        case id, stage, status
        case displayName = "display_name"
        case denyReason = "deny_reason"
        case poolCents = "pool_cents"
        case poolUnlockedCents = "pool_unlocked_cents"
        case poolPaidReferrerCents = "pool_paid_referrer_cents"
        case poolPaidRefereeCents = "pool_paid_referee_cents"
        case sharePaidCents = "share_paid_cents"
        case shareCapCents = "share_cap_cents"
        case earnedCents = "earned_cents"
        case daysLeft = "days_left"
        case createdAt = "created_at"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(Int.self, forKey: .id) ?? 0
        displayName = try c.decodeIfPresent(String.self, forKey: .displayName) ?? ""
        stage = try c.decodeIfPresent(String.self, forKey: .stage) ?? ""
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? ""
        denyReason = try c.decodeIfPresent(String.self, forKey: .denyReason) ?? ""
        poolCents = try c.decodeIfPresent(Int.self, forKey: .poolCents) ?? 0
        poolUnlockedCents = try c.decodeIfPresent(Int.self, forKey: .poolUnlockedCents) ?? 0
        poolPaidReferrerCents = try c.decodeIfPresent(Int.self, forKey: .poolPaidReferrerCents) ?? 0
        poolPaidRefereeCents = try c.decodeIfPresent(Int.self, forKey: .poolPaidRefereeCents) ?? 0
        sharePaidCents = try c.decodeIfPresent(Int.self, forKey: .sharePaidCents) ?? 0
        shareCapCents = try c.decodeIfPresent(Int.self, forKey: .shareCapCents) ?? 0
        earnedCents = try c.decodeIfPresent(Int.self, forKey: .earnedCents) ?? 0
        daysLeft = try c.decodeIfPresent(Int.self, forKey: .daysLeft) ?? 0
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
    }

    /// The stage in words.
    var stageTitle: String {
        switch stage {
        case "signed_up": "Signed up · waiting for their first top-up"
        case "active": daysLeft > 0 ? "Active · \(daysLeft) day\(daysLeft == 1 ? "" : "s") of share left" : "Active"
        case "completed": "Completed"
        case "denied": denyReason == "shared_device" ? "Not eligible · same computer as you" : "Not eligible · computer already rewarded"
        case "frozen": "On hold"
        default: stage
        }
    }
}

/// `gateway/referral`: the summary plus each referee.
struct GatewayReferral: Codable, Equatable {
    var summary: GatewayReferralSummary
    var referees: [GatewayReferee]

    init(from decoder: Decoder) throws {
        summary = try GatewayReferralSummary(from: decoder)
        let c = try decoder.container(keyedBy: CodingKeys.self)
        referees = try c.decodeIfPresent([GatewayReferee].self, forKey: .referees) ?? []
    }

    func encode(to encoder: Encoder) throws {
        try summary.encode(to: encoder)
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(referees, forKey: .referees)
    }

    enum CodingKeys: String, CodingKey {
        case referees
    }
}

/// `gateway/referral/update`'s reply.
struct GatewayReferralSplitResult: Codable, Equatable {
    var splitRefereeCents: Int
    var minSplitRefereeCents: Int
    var poolCents: Int

    enum CodingKeys: String, CodingKey {
        case splitRefereeCents = "split_referee_cents"
        case minSplitRefereeCents = "min_split_referee_cents"
        case poolCents = "pool_cents"
    }
}

/// `gateway/referral/attach`'s reply.
struct GatewayReferralAttachResult: Codable, Equatable {
    var invitedBy: GatewayInvitedBy

    enum CodingKeys: String, CodingKey {
        case invitedBy = "invited_by"
    }
}

/// One active ModelGateway plan of `gateway/account`. Amounts are quota
/// units (`amountTotal` 0 = unlimited); times are Unix seconds.
struct GatewaySubscription: Codable, Equatable, Identifiable {
    var id: Int
    var planId: Int
    var status: String
    var title: String
    var subtitle: String
    var priceAmount: Double
    var currency: String
    var durationUnit: String
    var durationValue: Int
    var quotaResetPeriod: String
    var amountTotal: Int
    var amountUsed: Int
    var amountReserved: Int
    var startTime: Int64
    var endTime: Int64
    var nextResetTime: Int64

    enum CodingKeys: String, CodingKey {
        case id, status, title, subtitle, currency
        case planId = "plan_id"
        case priceAmount = "price_amount"
        case durationUnit = "duration_unit"
        case durationValue = "duration_value"
        case quotaResetPeriod = "quota_reset_period"
        case amountTotal = "amount_total"
        case amountUsed = "amount_used"
        case amountReserved = "amount_reserved"
        case startTime = "start_time"
        case endTime = "end_time"
        case nextResetTime = "next_reset_time"
    }

    init(id: Int, planId: Int = 0, status: String = "active", title: String, subtitle: String = "", priceAmount: Double = 0, currency: String = "USD", durationUnit: String = "month", durationValue: Int = 1, quotaResetPeriod: String = "never", amountTotal: Int = 0, amountUsed: Int = 0, amountReserved: Int = 0, startTime: Int64 = 0, endTime: Int64 = 0, nextResetTime: Int64 = 0) {
        self.id = id
        self.planId = planId
        self.status = status
        self.title = title
        self.subtitle = subtitle
        self.priceAmount = priceAmount
        self.currency = currency
        self.durationUnit = durationUnit
        self.durationValue = durationValue
        self.quotaResetPeriod = quotaResetPeriod
        self.amountTotal = amountTotal
        self.amountUsed = amountUsed
        self.amountReserved = amountReserved
        self.startTime = startTime
        self.endTime = endTime
        self.nextResetTime = nextResetTime
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(Int.self, forKey: .id) ?? 0
        planId = try c.decodeIfPresent(Int.self, forKey: .planId) ?? 0
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? ""
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        subtitle = try c.decodeIfPresent(String.self, forKey: .subtitle) ?? ""
        priceAmount = try c.decodeIfPresent(Double.self, forKey: .priceAmount) ?? 0
        currency = try c.decodeIfPresent(String.self, forKey: .currency) ?? "USD"
        durationUnit = try c.decodeIfPresent(String.self, forKey: .durationUnit) ?? ""
        durationValue = try c.decodeIfPresent(Int.self, forKey: .durationValue) ?? 0
        quotaResetPeriod = try c.decodeIfPresent(String.self, forKey: .quotaResetPeriod) ?? "never"
        amountTotal = try c.decodeIfPresent(Int.self, forKey: .amountTotal) ?? 0
        amountUsed = try c.decodeIfPresent(Int.self, forKey: .amountUsed) ?? 0
        amountReserved = try c.decodeIfPresent(Int.self, forKey: .amountReserved) ?? 0
        startTime = try c.decodeIfPresent(Int64.self, forKey: .startTime) ?? 0
        endTime = try c.decodeIfPresent(Int64.self, forKey: .endTime) ?? 0
        nextResetTime = try c.decodeIfPresent(Int64.self, forKey: .nextResetTime) ?? 0
    }

    /// The plan title, else "Plan #id" (the plan was deleted).
    var name: String {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Plan #\(planId)" : trimmed
    }

    var isUnlimited: Bool { amountTotal <= 0 }
    var remaining: Int { max(0, amountTotal - amountUsed - amountReserved) }
    /// Used share of the plan's quota, 0…1 (0 when unlimited).
    var usedFraction: Double {
        isUnlimited ? 0 : min(1, Double(amountUsed + amountReserved) / Double(amountTotal))
    }

    var endDate: Date? { endTime > 0 ? Date(timeIntervalSince1970: TimeInterval(endTime)) : nil }
    var nextResetDate: Date? { nextResetTime > 0 ? Date(timeIntervalSince1970: TimeInterval(nextResetTime)) : nil }

    /// Whole days until the plan ends (0 on its last day).
    func daysLeft(now: Date = Date()) -> Int? {
        guard let end = endDate else { return nil }
        return max(0, Int(end.timeIntervalSince(now) / 86_400))
    }

    /// "$20 / month", "$50 / 3 months", "Free"; nil when the plan is unknown.
    var priceTitle: String? {
        guard !title.isEmpty else { return nil }
        let price = priceAmount > 0 ? priceAmount.formatted(.currency(code: currency.isEmpty ? "USD" : currency)) : "Free"
        guard priceAmount > 0, ["year", "month", "day", "hour"].contains(durationUnit), durationValue > 0 else { return price }
        return durationValue == 1 ? "\(price) / \(durationUnit)" : "\(price) / \(durationValue) \(durationUnit)s"
    }

    /// "Resets monthly", nil when the quota never resets.
    var resetTitle: String? {
        switch quotaResetPeriod {
        case "daily", "weekly", "monthly": "Resets \(quotaResetPeriod)"
        case "custom": "Resets periodically"
        default: nil
        }
    }
}

/// The API key the gateway created for this device.
struct GatewayToken: Codable, Equatable {
    var id: Int
    var name: String
    var usedQuota: Int
    var createdTime: Int64

    enum CodingKeys: String, CodingKey {
        case id, name
        case usedQuota = "used_quota"
        case createdTime = "created_time"
    }
}

/// `gateway/device/start`.
struct GatewayDeviceStart: Codable, Equatable {
    var deviceCode: String
    var userCode: String
    var verificationUri: String
    var verificationUriComplete: String
    var expiresIn: Int
    var interval: Int

    enum CodingKeys: String, CodingKey {
        case interval
        case deviceCode = "device_code"
        case userCode = "user_code"
        case verificationUri = "verification_uri"
        case verificationUriComplete = "verification_uri_complete"
        case expiresIn = "expires_in"
    }
}

/// `gateway/device/poll`: `pending` · `approved` · `denied` · `expired`.
/// The key itself stays in the backend's credential store.
struct GatewayDevicePoll: Codable, Equatable {
    var status: String
    var interval: Int?
    var account: GatewayAccount?
}

/// `gateway/status`: whether the backend holds a ModelGateway device key.
struct GatewayStatus: Codable, Equatable {
    var signedIn: Bool
    var origin: String?
    var account: String?

    enum CodingKeys: String, CodingKey {
        case origin, account
        case signedIn = "signed_in"
    }
}

/// `gateway/signout`.
struct GatewaySignOutResult: Codable, Equatable {
    var revoked: Bool
}

/// One hourly usage row of `gateway/usage` (`created_at` is the hour, UTC).
struct GatewayUsageRow: Codable, Equatable {
    var createdAt: Int64
    var modelName: String
    var project: String
    var count: Int
    var quota: Int
    var tokenUsed: Int

    enum CodingKeys: String, CodingKey {
        case project, count, quota
        case createdAt = "created_at"
        case modelName = "model_name"
        case tokenUsed = "token_used"
    }

    init(createdAt: Int64, modelName: String, project: String, count: Int, quota: Int, tokenUsed: Int) {
        self.createdAt = createdAt
        self.modelName = modelName
        self.project = project
        self.count = count
        self.quota = quota
        self.tokenUsed = tokenUsed
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
        modelName = try c.decodeIfPresent(String.self, forKey: .modelName) ?? ""
        project = try c.decodeIfPresent(String.self, forKey: .project) ?? ""
        count = try c.decodeIfPresent(Int.self, forKey: .count) ?? 0
        quota = try c.decodeIfPresent(Int.self, forKey: .quota) ?? 0
        tokenUsed = try c.decodeIfPresent(Int.self, forKey: .tokenUsed) ?? 0
    }
}

/// `gateway/usage`.
struct GatewayUsageReport: Codable, Equatable {
    var startTimestamp: Int64
    var endTimestamp: Int64
    var quotaPerUnit: Double
    var displayInCurrency: Bool
    var rows: [GatewayUsageRow]

    enum CodingKeys: String, CodingKey {
        case rows
        case startTimestamp = "start_timestamp"
        case endTimestamp = "end_timestamp"
        case quotaPerUnit = "quota_per_unit"
        case displayInCurrency = "display_in_currency"
    }

    init(startTimestamp: Int64, endTimestamp: Int64, quotaPerUnit: Double = 500_000, displayInCurrency: Bool = true, rows: [GatewayUsageRow]) {
        self.startTimestamp = startTimestamp
        self.endTimestamp = endTimestamp
        self.quotaPerUnit = quotaPerUnit
        self.displayInCurrency = displayInCurrency
        self.rows = rows
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        startTimestamp = try c.decodeIfPresent(Int64.self, forKey: .startTimestamp) ?? 0
        endTimestamp = try c.decodeIfPresent(Int64.self, forKey: .endTimestamp) ?? 0
        quotaPerUnit = try c.decodeIfPresent(Double.self, forKey: .quotaPerUnit) ?? 500_000
        displayInCurrency = try c.decodeIfPresent(Bool.self, forKey: .displayInCurrency) ?? true
        rows = try c.decodeIfPresent([GatewayUsageRow].self, forKey: .rows) ?? []
    }
}

// MARK: - Usage maths

/// Spend, tokens and requests of one day (or of a whole series).
struct GatewayUsageTotals: Equatable, Sendable {
    var quota = 0
    var tokens = 0
    var requests = 0

    var isEmpty: Bool { quota == 0 && tokens == 0 && requests == 0 }

    mutating func add(_ row: GatewayUsageRow) {
        quota += row.quota
        tokens += row.tokenUsed
        requests += row.count
    }

    mutating func add(_ other: GatewayUsageTotals) {
        quota += other.quota
        tokens += other.tokens
        requests += other.requests
    }
}

/// What a heatmap cell measures.
enum UsageMetric: String, CaseIterable, Identifiable {
    case cost, tokens, requests

    var id: String { rawValue }

    var title: String {
        switch self {
        case .cost: return "Cost"
        case .tokens: return "Tokens"
        case .requests: return "Requests"
        }
    }

    func value(_ totals: GatewayUsageTotals, quotaPerUnit: Double) -> Double {
        switch self {
        case .cost: return Double(totals.quota) / max(quotaPerUnit, 1)
        case .tokens: return Double(totals.tokens)
        case .requests: return Double(totals.requests)
        }
    }

    func format(_ totals: GatewayUsageTotals, quotaPerUnit: Double) -> String {
        switch self {
        case .cost: return UsageFormat.usd(value(totals, quotaPerUnit: quotaPerUnit))
        case .tokens: return UsageFormat.compact(totals.tokens) + " tokens"
        case .requests: return UsageFormat.count(totals.requests, singular: "request", plural: "requests")
        }
    }
}

/// Total · By model · By project.
enum UsageScope: String, CaseIterable, Identifiable {
    case total, model, project

    var id: String { rawValue }

    var title: String {
        switch self {
        case .total: return "Total"
        case .model: return "By model"
        case .project: return "By project"
        }
    }
}

/// The Usage card's period: 24h · 7d · 30d · All. "All" is the whole
/// report (`GatewayState.usageDays`) as a heatmap of days; the others are
/// bar charts of their last `bucketCount` hours or days, the current one
/// included.
enum UsageRange: String, CaseIterable, Identifiable, Sendable {
    case day, week, month, all

    var id: String { rawValue }

    var title: String {
        switch self {
        case .day: return "24h"
        case .week: return "7d"
        case .month: return "30d"
        case .all: return "All"
        }
    }

    /// "Last 24 hours", for captions.
    var caption: String {
        switch self {
        case .day: return "Last 24 hours"
        case .week: return "Last 7 days"
        case .month: return "Last 30 days"
        case .all: return "Last 12 months"
        }
    }

    /// What one bar or heatmap cell covers.
    var bucket: Calendar.Component { self == .day ? .hour : .day }

    /// Bars in the chart; nil for the heatmap.
    var bucketCount: Int? {
        switch self {
        case .day: return 24
        case .week: return 7
        case .month: return 30
        case .all: return nil
        }
    }

    /// The bucket a moment falls in (its local hour or day).
    func bucketStart(of date: Date, calendar: Calendar) -> Date {
        bucket == .hour ? (calendar.dateInterval(of: .hour, for: date)?.start ?? date) : calendar.startOfDay(for: date)
    }

    /// The chart's buckets, oldest first, ending with the one `now` is in;
    /// empty for `.all`.
    func buckets(now: Date, calendar: Calendar) -> [Date] {
        guard let count = bucketCount else { return [] }
        let last = bucketStart(of: now, calendar: calendar)
        return (0..<count).reversed().compactMap { calendar.date(byAdding: bucket, value: -$0, to: last) }
    }
}

/// One chart's data: totals per bucket (a local day, or an hour for the
/// 24-hour range) plus the sum.
struct UsageSeries: Identifiable, Equatable, Sendable {
    let id: String
    let title: String
    var buckets: [Date: GatewayUsageTotals] = [:]
    var total = GatewayUsageTotals()

    init(id: String, title: String) {
        self.id = id
        self.title = title
    }

    mutating func add(_ row: GatewayUsageRow, bucket: Date) {
        buckets[bucket, default: GatewayUsageTotals()].add(row)
        total.add(row)
    }
}

/// The Account page's three views of one usage report over one range.
/// Hours are folded into buckets of the given calendar (the user's, by
/// default) so a day on the chart is a local day.
struct UsageBreakdown: Equatable, Sendable {
    static let unattributed = "Unattributed"

    var range: UsageRange
    /// The bar chart's buckets, oldest first (empty for the heatmap).
    var bucketStarts: [Date]
    var total: UsageSeries
    var models: [UsageSeries]
    var projects: [UsageSeries]
    var quotaPerUnit: Double
    var displayInCurrency: Bool
    /// Rows in the whole report, whatever the range.
    var rowCount: Int

    init(report: GatewayUsageReport, range: UsageRange = .all, now: Date = Date(), calendar: Calendar = .current) {
        let bucketStarts = range.buckets(now: now, calendar: calendar)
        let since = bucketStarts.first.map { Int64($0.timeIntervalSince1970) } ?? .min
        var total = UsageSeries(id: "total", title: "All usage")
        var models: [String: UsageSeries] = [:]
        var projects: [String: UsageSeries] = [:]
        var bucketCache: [Int64: Date] = [:]
        for row in report.rows where row.createdAt >= since {
            // Rows are hourly; every hour of one bucket maps to the same
            // bucket, so the lookup is cached per hour stamp.
            let bucket: Date
            if let cached = bucketCache[row.createdAt] {
                bucket = cached
            } else {
                bucket = range.bucketStart(of: Date(timeIntervalSince1970: TimeInterval(row.createdAt)), calendar: calendar)
                bucketCache[row.createdAt] = bucket
            }
            total.add(row, bucket: bucket)
            let model = row.modelName.isEmpty ? "(unknown model)" : row.modelName
            models[model, default: UsageSeries(id: "model:" + model, title: model)].add(row, bucket: bucket)
            let project = row.project.isEmpty ? Self.unattributed : row.project
            projects[project, default: UsageSeries(id: "project:" + project, title: project)].add(row, bucket: bucket)
        }
        self.range = range
        self.bucketStarts = bucketStarts
        self.total = total
        self.models = Self.sorted(models)
        self.projects = Self.sorted(projects)
        quotaPerUnit = report.quotaPerUnit
        displayInCurrency = report.displayInCurrency
        rowCount = report.rows.count
    }

    /// One breakdown per range, as the Usage card's picker switches between them.
    static func byRange(report: GatewayUsageReport, now: Date = Date(), calendar: Calendar = .current) -> [UsageRange: UsageBreakdown] {
        Dictionary(uniqueKeysWithValues: UsageRange.allCases.map { ($0, UsageBreakdown(report: report, range: $0, now: now, calendar: calendar)) })
    }

    /// Biggest spender first, the unattributed bucket last among equals.
    private static func sorted(_ series: [String: UsageSeries]) -> [UsageSeries] {
        series.values.sorted { a, b in
            if a.total.quota != b.total.quota { return a.total.quota > b.total.quota }
            if a.total.requests != b.total.requests { return a.total.requests > b.total.requests }
            if (a.title == unattributed) != (b.title == unattributed) { return b.title == unattributed }
            return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
        }
    }

    func series(for scope: UsageScope) -> [UsageSeries] {
        switch scope {
        case .total: return [total]
        case .model: return models
        case .project: return projects
        }
    }
}

/// Number formatting shared by the Account page and the sidebar button.
enum UsageFormat {
    /// "$12.34"; tiny non-zero amounts show three decimals ("$0.004"),
    /// zero is "$0.00".
    static func usd(_ value: Double) -> String {
        if value == 0 { return "$0.00" }
        if abs(value) < 0.01 { return String(format: "$%.3f", value) }
        if abs(value) >= 1000 { return "$" + compact(Int(value.rounded())) }
        return String(format: "$%.2f", value)
    }

    /// "12.3k", "1.2M", "845".
    static func compact(_ value: Int) -> String {
        let n = Double(value)
        switch abs(value) {
        case 0..<1000: return "\(value)"
        case 1000..<1_000_000: return trimmed(n / 1000) + "k"
        case 1_000_000..<1_000_000_000: return trimmed(n / 1_000_000) + "M"
        default: return trimmed(n / 1_000_000_000) + "B"
        }
    }

    static func count(_ value: Int, singular: String, plural: String) -> String {
        "\(value.formatted()) \(value == 1 ? singular : plural)"
    }

    private static func trimmed(_ value: Double) -> String {
        let text = String(format: value < 10 ? "%.1f" : "%.0f", value)
        return text.hasSuffix(".0") ? String(text.dropLast(2)) : text
    }
}

// MARK: - Billing (`gateway/billing*`)

/// How requests draw on a plan and the balance (`billing_preference`).
enum GatewayBillingPreference: String, CaseIterable, Identifiable {
    case subscriptionFirst = "subscription_first"
    case walletFirst = "wallet_first"
    case subscriptionOnly = "subscription_only"
    case walletOnly = "wallet_only"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .subscriptionFirst: "Subscription first, then credits"
        case .walletFirst: "Credits first, then subscription"
        case .subscriptionOnly: "Subscription only"
        case .walletOnly: "Credits only"
        }
    }

    var detail: String {
        switch self {
        case .subscriptionFirst: "Requests use your plan's allowance; once it runs out they are paid from your credit balance."
        case .walletFirst: "Requests are paid from your credit balance; once it is empty they use your plan's allowance."
        case .subscriptionOnly: "Requests only use your plan's allowance and fail once it runs out."
        case .walletOnly: "Requests are only paid from your credit balance; your plan is not used."
        }
    }
}

/// `gateway/billing`: what the Billing settings show.
struct GatewayBilling: Codable, Equatable {
    var billingPreference: String
    var quotaPerUnit: Double
    var displayInCurrency: Bool
    var walletURL: String?
    var topUp: GatewayTopUpOptions
    var plans: [GatewayPlan]
    var hasActiveSubscription: Bool

    enum CodingKeys: String, CodingKey {
        case plans
        case billingPreference = "billing_preference"
        case quotaPerUnit = "quota_per_unit"
        case displayInCurrency = "display_in_currency"
        case walletURL = "wallet_url"
        case topUp = "topup"
        case hasActiveSubscription = "has_active_subscription"
    }

    init(billingPreference: String = "subscription_first", quotaPerUnit: Double = 500_000, displayInCurrency: Bool = true, walletURL: String? = nil, topUp: GatewayTopUpOptions = GatewayTopUpOptions(), plans: [GatewayPlan] = [], hasActiveSubscription: Bool = false) {
        self.billingPreference = billingPreference
        self.quotaPerUnit = quotaPerUnit
        self.displayInCurrency = displayInCurrency
        self.walletURL = walletURL
        self.topUp = topUp
        self.plans = plans
        self.hasActiveSubscription = hasActiveSubscription
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        billingPreference = try c.decodeIfPresent(String.self, forKey: .billingPreference) ?? "subscription_first"
        quotaPerUnit = try c.decodeIfPresent(Double.self, forKey: .quotaPerUnit) ?? 500_000
        displayInCurrency = try c.decodeIfPresent(Bool.self, forKey: .displayInCurrency) ?? true
        walletURL = try c.decodeIfPresent(String.self, forKey: .walletURL)
        topUp = try c.decodeIfPresent(GatewayTopUpOptions.self, forKey: .topUp) ?? GatewayTopUpOptions()
        plans = try c.decodeIfPresent([GatewayPlan].self, forKey: .plans) ?? []
        hasActiveSubscription = try c.decodeIfPresent(Bool.self, forKey: .hasActiveSubscription) ?? false
    }

    var preference: GatewayBillingPreference? { GatewayBillingPreference(rawValue: billingPreference) }
}

/// Adding funds to the balance. Amounts are dollars (`amount_unit` "usd")
/// or raw quota units ("quota").
struct GatewayTopUpOptions: Codable, Equatable {
    var enabled: Bool = false
    /// A checkout link can be made from the app (else: the wallet page).
    var inApp: Bool = false
    var minAmount: Int = 1
    var maxAmount: Int = 10_000
    var amountOptions: [Int] = []
    /// Amount (as a string) → price multiplier.
    var discounts: [String: Double] = [:]
    var amountUnit: String = "usd"

    enum CodingKeys: String, CodingKey {
        case enabled, discounts
        case inApp = "in_app"
        case minAmount = "min_amount"
        case maxAmount = "max_amount"
        case amountOptions = "amount_options"
        case amountUnit = "amount_unit"
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        inApp = try c.decodeIfPresent(Bool.self, forKey: .inApp) ?? false
        minAmount = try c.decodeIfPresent(Int.self, forKey: .minAmount) ?? 1
        maxAmount = try c.decodeIfPresent(Int.self, forKey: .maxAmount) ?? 10_000
        amountOptions = try c.decodeIfPresent([Int].self, forKey: .amountOptions) ?? []
        discounts = try c.decodeIfPresent([String: Double].self, forKey: .discounts) ?? [:]
        amountUnit = try c.decodeIfPresent(String.self, forKey: .amountUnit) ?? "usd"
    }

    var isUSD: Bool { amountUnit != "quota" }

    /// "$20" or "20,000 units".
    func label(_ amount: Int) -> String {
        isUSD ? UsageFormat.usd(Double(amount)) : "\(amount.formatted()) units"
    }

    /// The discount on `amount`, as "5% off"; nil when none.
    func discountLabel(_ amount: Int) -> String? {
        guard let multiplier = discounts[String(amount)], multiplier > 0, multiplier < 1 else { return nil }
        return "\(Int(((1 - multiplier) * 100).rounded()))% off"
    }
}

/// A plan on sale.
struct GatewayPlan: Codable, Equatable, Identifiable {
    var id: Int
    var title: String
    var subtitle: String
    var priceAmount: Double
    var currency: String
    var durationUnit: String
    var durationValue: Int
    var quotaResetPeriod: String
    /// Quota units; 0 = unlimited.
    var totalAmount: Int
    var stripe: Bool
    var balance: Bool

    enum CodingKeys: String, CodingKey {
        case id, title, subtitle, currency, stripe, balance
        case priceAmount = "price_amount"
        case durationUnit = "duration_unit"
        case durationValue = "duration_value"
        case quotaResetPeriod = "quota_reset_period"
        case totalAmount = "total_amount"
    }

    init(id: Int, title: String, subtitle: String = "", priceAmount: Double, currency: String = "USD", durationUnit: String = "month", durationValue: Int = 1, quotaResetPeriod: String = "monthly", totalAmount: Int = 0, stripe: Bool = true, balance: Bool = true) {
        self.id = id
        self.title = title
        self.subtitle = subtitle
        self.priceAmount = priceAmount
        self.currency = currency
        self.durationUnit = durationUnit
        self.durationValue = durationValue
        self.quotaResetPeriod = quotaResetPeriod
        self.totalAmount = totalAmount
        self.stripe = stripe
        self.balance = balance
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(Int.self, forKey: .id) ?? 0
        title = try c.decodeIfPresent(String.self, forKey: .title) ?? ""
        subtitle = try c.decodeIfPresent(String.self, forKey: .subtitle) ?? ""
        priceAmount = try c.decodeIfPresent(Double.self, forKey: .priceAmount) ?? 0
        currency = try c.decodeIfPresent(String.self, forKey: .currency) ?? "USD"
        durationUnit = try c.decodeIfPresent(String.self, forKey: .durationUnit) ?? ""
        durationValue = try c.decodeIfPresent(Int.self, forKey: .durationValue) ?? 0
        quotaResetPeriod = try c.decodeIfPresent(String.self, forKey: .quotaResetPeriod) ?? "never"
        totalAmount = try c.decodeIfPresent(Int.self, forKey: .totalAmount) ?? 0
        stripe = try c.decodeIfPresent(Bool.self, forKey: .stripe) ?? false
        balance = try c.decodeIfPresent(Bool.self, forKey: .balance) ?? false
    }

    var name: String { title.isEmpty ? "Plan #\(id)" : title }

    /// "$20 / month", "Free".
    var priceTitle: String {
        let price = priceAmount > 0 ? priceAmount.formatted(.currency(code: currency.isEmpty ? "USD" : currency)) : "Free"
        guard priceAmount > 0, ["year", "month", "day", "hour"].contains(durationUnit), durationValue > 0 else { return price }
        return durationValue == 1 ? "\(price) / \(durationUnit)" : "\(price) / \(durationValue) \(durationUnit)s"
    }

    /// "$10 of usage, resets monthly" / "Unlimited usage".
    func allowanceTitle(quotaPerUnit: Double) -> String {
        let reset: String = switch quotaResetPeriod {
        case "daily", "weekly", "monthly": ", resets \(quotaResetPeriod)"
        default: ""
        }
        guard totalAmount > 0 else { return "Unlimited usage" + reset }
        return UsageFormat.usd(Double(totalAmount) / max(quotaPerUnit, 1)) + " of usage" + reset
    }
}

struct GatewayTopUpQuote: Codable, Equatable {
    var amount: Double
    var payMoney: Double
    var currency: String?

    enum CodingKeys: String, CodingKey {
        case amount, currency
        case payMoney = "pay_money"
    }
}

/// `gateway/billing/topup` and `/subscribe`: a page to pay on, or done.
struct GatewayCheckout: Codable, Equatable {
    var checkoutURL: String?
    var subscribed: Bool?

    enum CodingKeys: String, CodingKey {
        case subscribed
        case checkoutURL = "checkout_url"
    }
}

struct GatewayPreferenceResult: Codable, Equatable {
    var billingPreference: String

    enum CodingKeys: String, CodingKey {
        case billingPreference = "billing_preference"
    }
}

// MARK: - Device key settings

/// One price rule of the device key: requests only go to providers charging
/// at most `100 − discountPercent` percent of the model's standard
/// output-token price, and at most `maxPricePerM` dollars per million
/// output tokens. Either may be absent.
struct GatewayPriceRule: Codable, Equatable {
    var discountPercent: Double?
    var maxPricePerM: Double?

    enum CodingKeys: String, CodingKey {
        case discountPercent = "discount_percent"
        case maxPricePerM = "max_price_per_m"
    }

    /// The share of the standard price the key pays at most, 0–100.
    var paidPercent: Double? { discountPercent.map { 100 - $0 } }

    var isEmpty: Bool { discountPercent == nil && maxPricePerM == nil }

    var json: JSONValue {
        var object: [String: JSONValue] = [:]
        if let discountPercent { object["discount_percent"] = .number(discountPercent) }
        if let maxPricePerM { object["max_price_per_m"] = .number(maxPricePerM) }
        return .object(object)
    }
}

/// The key-wide rule and the per-model rules that replace it.
struct GatewayPriceLimits: Codable, Equatable {
    var `default`: GatewayPriceRule?
    var models: [String: GatewayPriceRule] = [:]

    init(default: GatewayPriceRule? = nil, models: [String: GatewayPriceRule] = [:]) {
        self.default = `default`
        self.models = models
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        `default` = try c.decodeIfPresent(GatewayPriceRule.self, forKey: .default)
        models = try c.decodeIfPresent([String: GatewayPriceRule].self, forKey: .models) ?? [:]
    }

    /// The `price_limits` body of `gateway/key/update` (`{}` clears).
    var json: JSONValue {
        var object: [String: JSONValue] = [:]
        if let rule = `default`, !rule.isEmpty { object["default"] = rule.json }
        let rules = models.filter { !$0.value.isEmpty }
        if !rules.isEmpty { object["models"] = .object(rules.mapValues(\.json)) }
        return .object(object)
    }
}

/// `gateway/key`: the signed-in device key's price limits and spending
/// cap, the settings the website's key drawer edits for ordinary keys.
struct GatewayKeySettings: Codable, Equatable {
    var name: String
    var exhausted: Bool
    var unlimitedQuota: Bool
    var remainQuota: Int
    var usedQuota: Int
    var quotaPerUnit: Double
    var displayInCurrency: Bool
    var priceLimits: GatewayPriceLimits

    enum CodingKeys: String, CodingKey {
        case name, exhausted
        case unlimitedQuota = "unlimited_quota"
        case remainQuota = "remain_quota"
        case usedQuota = "used_quota"
        case quotaPerUnit = "quota_per_unit"
        case displayInCurrency = "display_in_currency"
        case priceLimits = "price_limits"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
        exhausted = try c.decodeIfPresent(Bool.self, forKey: .exhausted) ?? false
        unlimitedQuota = try c.decodeIfPresent(Bool.self, forKey: .unlimitedQuota) ?? true
        remainQuota = try c.decodeIfPresent(Int.self, forKey: .remainQuota) ?? 0
        usedQuota = try c.decodeIfPresent(Int.self, forKey: .usedQuota) ?? 0
        quotaPerUnit = try c.decodeIfPresent(Double.self, forKey: .quotaPerUnit) ?? 500_000
        displayInCurrency = try c.decodeIfPresent(Bool.self, forKey: .displayInCurrency) ?? true
        priceLimits = try c.decodeIfPresent(GatewayPriceLimits.self, forKey: .priceLimits) ?? GatewayPriceLimits()
    }

    /// A quota amount as the gateway shows it: dollars, or raw units.
    func formatQuota(_ amount: Int) -> String {
        displayInCurrency ? UsageFormat.usd(Double(amount) / max(quotaPerUnit, 1)) : UsageFormat.compact(amount)
    }
}
