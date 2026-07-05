//! Turning bank CSV exports into a common shape.
//!
//! Every bank exports a different dialect — different delimiters, encodings,
//! number and date formats, and a pile of preamble lines before the real header.
//! This module hides those differences behind [`parse`], which sniffs the format
//! and returns a flat list of [`ParsedRow`]s plus any per-row errors. The database
//! side (`commands::import_csv_into`) stays oblivious to which bank a file came
//! from.
//!
//! Adding a bank is a new [`BankFormat`] arm, a `parse_*` function, and a signature
//! in [`detect_format`] — no schema or UI change.

use chrono::NaiveDate;

/// One transaction lifted out of a CSV, normalized to the app's conventions:
/// ISO date, integer cents, and a trimmed counterparty ("concept").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedRow {
    pub date: String,
    /// Amount in cents; negative = expense.
    pub amount_cents: i64,
    /// The payee/counterparty that drives auto-classification. May be empty.
    pub counterparty: String,
    pub description: String,
    /// An explicit category named in the file (canonical format only). Bank
    /// exports leave this `None` and rely on learned classification rules.
    pub category: Option<String>,
    /// A stable per-transaction reference from the bank (e.g. comdirect's
    /// `Referenz`), when the export carries one. Empty otherwise. Used as the
    /// duplicate-detection identity so two genuinely distinct charges that share a
    /// date, amount, and merchant aren't collapsed into one.
    pub import_ref: String,
}

/// The CSV dialects we know how to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BankFormat {
    /// The app's own `date,amount,description,category` template.
    Canonical,
    /// ING-DiBa Girokonto "Umsatzanzeige" export.
    IngDiba,
    /// ING España "Movimientos de la Cuenta" export.
    IngEs,
    /// comdirect account "Umsätze" export.
    Comdirect,
    /// Deutsche Bank / maxblue account "Umsätze" export.
    DeutscheBank,
    /// DEGIRO (Spanish) cash-account "Account.csv" export.
    Degiro,
}

impl BankFormat {
    /// A human-readable name for surfacing the detected format in the UI.
    pub(crate) fn label(self) -> &'static str {
        match self {
            BankFormat::Canonical => "Canonical template",
            BankFormat::IngDiba => "ING-DiBa",
            BankFormat::IngEs => "ING España",
            BankFormat::Comdirect => "comdirect",
            BankFormat::DeutscheBank => "Deutsche Bank",
            BankFormat::Degiro => "DEGIRO",
        }
    }
}

/// Guess a file's format from its content.
pub(crate) fn detect_format(text: &str) -> BankFormat {
    for line in text.lines() {
        let t = line.trim();
        // ING's data header, or its file banner.
        if t.starts_with("Buchung;Wertstellungsdatum") || t.starts_with("Umsatzanzeige;") {
            return BankFormat::IngDiba;
        }
        // ING España's `,`-delimited data header. (Its banner line carries a UTF-8
        // BOM that `trim` doesn't strip, so we key off the clean data header.)
        if t.starts_with("F. VALOR,") {
            return BankFormat::IngEs;
        }
        // comdirect's quoted data header, or its "Umsätze <Konto>" banner.
        if t.starts_with("\"Buchungstag\"") || t.starts_with("\"Umsätze") {
            return BankFormat::Comdirect;
        }
        // Deutsche Bank / maxblue's unquoted data header. (comdirect quotes the same
        // leading column, so the quote check above wins for those files.)
        if t.starts_with("Buchungstag;Wert;Umsatzart") {
            return BankFormat::DeutscheBank;
        }
        // DEGIRO's Spanish cash-account data header. The amount and balance columns
        // carry no name, so key off the fixed leading columns.
        if t.starts_with("Fecha,Hora,Fecha valor") {
            return BankFormat::Degiro;
        }
    }
    BankFormat::Canonical
}

/// Parse a decoded CSV document.
///
/// `Err` is a fatal, whole-file problem (unrecognized/unreadable header). `Ok`
/// carries the successfully parsed rows plus a per-row error for each line that
/// could not be read — mirroring the "report bad rows but keep going" behavior of
/// the original importer.
pub(crate) fn parse(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    match detect_format(text) {
        BankFormat::IngDiba => parse_ing(text),
        BankFormat::IngEs => parse_ing_es(text),
        BankFormat::Comdirect => parse_comdirect(text),
        BankFormat::DeutscheBank => parse_db(text),
        BankFormat::Degiro => parse_degiro(text),
        BankFormat::Canonical => parse_canonical(text),
    }
}

// ----------------------------------------------------------------------------
// Canonical (the app's own template)
// ----------------------------------------------------------------------------

/// `date,amount,description,category` with header-matched, order-independent
/// columns. Amounts use a `.` decimal separator; `category` is optional.
fn parse_canonical(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(text.as_bytes());

    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let idx = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let date_i = idx("date").ok_or("CSV is missing a 'date' column")?;
    let amount_i = idx("amount").ok_or("CSV is missing an 'amount' column")?;
    let desc_i = idx("description").ok_or("CSV is missing a 'description' column")?;
    let cat_i = idx("category");

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let line = n + 2; // +1 header, +1 for 1-based
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let date = match normalize_date(record.get(date_i).unwrap_or("").trim()) {
            Ok(d) => d,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        let amount_cents = match parse_amount_cents(record.get(amount_i).unwrap_or("").trim()) {
            Ok(a) => a,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        let category = cat_i
            .and_then(|i| record.get(i))
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string);

        rows.push(ParsedRow {
            date,
            amount_cents,
            counterparty: String::new(),
            description: record.get(desc_i).unwrap_or("").trim().to_string(),
            category,
            import_ref: String::new(),
        });
    }
    Ok((rows, errors))
}

// ----------------------------------------------------------------------------
// ING-DiBa Girokonto
// ----------------------------------------------------------------------------

/// ING-DiBa "Umsatzanzeige": `;`-delimited, German numbers (`-5.000,00`) and dates
/// (`22.06.2026`), preceded by a metadata preamble. The counterparty comes from the
/// "Auftraggeber/Empfänger" column; the description joins "Buchungstext" and
/// "Verwendungszweck".
fn parse_ing(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    // The data table starts at the "Buchung;Wertstellungsdatum;…" line; everything
    // above it is account metadata.
    let header_pos = text
        .lines()
        .position(|l| l.trim().starts_with("Buchung;Wertstellungsdatum"))
        .ok_or("Could not find the ING transaction table header")?;
    let data: String = text.lines().skip(header_pos).collect::<Vec<_>>().join("\n");

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(data.as_bytes());

    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let idx = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let date_i = idx("Buchung").ok_or("ING CSV is missing the 'Buchung' column")?;
    let amount_i = idx("Betrag").ok_or("ING CSV is missing the 'Betrag' column")?;
    let payee_i = idx("Auftraggeber/Empfänger");
    let text_i = idx("Buchungstext");
    let purpose_i = idx("Verwendungszweck");

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let line = n + 2;
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let date = match normalize_date(record.get(date_i).unwrap_or("").trim()) {
            Ok(d) => d,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        let amount_cents = match parse_de_amount_cents(record.get(amount_i).unwrap_or("").trim()) {
            Ok(a) => a,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let counterparty =
            normalize_concept(payee_i.and_then(|i| record.get(i)).unwrap_or("").trim());
        let get = |i: Option<usize>| i.and_then(|i| record.get(i)).unwrap_or("").trim();
        let description = [get(text_i), get(purpose_i)]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");

        rows.push(ParsedRow {
            date,
            amount_cents,
            counterparty,
            description,
            category: None,
            // The ING Girokonto export has no dedicated reference column; its
            // Verwendungszweck already distinguishes otherwise-identical rows.
            import_ref: String::new(),
        });
    }
    Ok((rows, errors))
}

// ----------------------------------------------------------------------------
// ING España
// ----------------------------------------------------------------------------

/// ING España "Movimientos de la Cuenta": `,`-delimited, US-style numbers (`.`
/// decimal, e.g. `-14.52`) and mixed dates (`14/04/2026` alongside the short
/// `4/6/26`), preceded by a metadata preamble (account number, holder, export
/// date). Unlike the German ING-DiBa export there is no counterparty column, so the
/// free-text `DESCRIPCIÓN` drives both the description and the classification
/// concept. The file also carries ING's own `CATEGORÍA`, but we leave the app's
/// category unset and rely on learned rules, mirroring the other bank importers.
fn parse_ing_es(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    // The data table starts at the "F. VALOR,CATEGORÍA,…" header; everything above
    // it is account metadata (banner, account number, holder, export date).
    let header_pos = text
        .lines()
        .position(|l| l.trim().starts_with("F. VALOR,"))
        .ok_or("Could not find the ING España transaction table header")?;
    let data: String = text.lines().skip(header_pos).collect::<Vec<_>>().join("\n");

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(data.as_bytes());

    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let idx = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let date_i = idx("F. VALOR").ok_or("ING España CSV is missing the 'F. VALOR' column")?;
    let amount_i =
        idx("IMPORTE (€)").ok_or("ING España CSV is missing the 'IMPORTE (€)' column")?;
    let desc_i = idx("DESCRIPCIÓN").ok_or("ING España CSV is missing the 'DESCRIPCIÓN' column")?;

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let line = n + 2;
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let date = match normalize_es_date(record.get(date_i).unwrap_or("").trim()) {
            Ok(d) => d,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        // Amounts use a `.` decimal separator, like the canonical template.
        let amount_cents = match parse_amount_cents(record.get(amount_i).unwrap_or("").trim()) {
            Ok(a) => a,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let description = normalize_concept(record.get(desc_i).unwrap_or("").trim());

        rows.push(ParsedRow {
            date,
            amount_cents,
            // No dedicated counterparty column; the free-text description is the best
            // available classification concept.
            counterparty: description.clone(),
            description,
            category: None,
            // No per-transaction reference in the export; fall back to
            // date+amount+description for dedup, as the German ING importer does.
            import_ref: String::new(),
        });
    }
    Ok((rows, errors))
}

// ----------------------------------------------------------------------------
// DEGIRO (Spanish cash account)
// ----------------------------------------------------------------------------

/// DEGIRO's Spanish "Account.csv" cash statement: `,`-delimited, European numbers
/// (`"-51,18"` — comma decimal, quoted) and `DD-MM-YYYY` dates. Two columns carry no
/// header name: the amount is the field right after `Variación` (which holds the
/// amount's currency), and the running balance the field after `Saldo`. We read those
/// positionally rather than by name.
///
/// The account is EUR-denominated, but the file also lists USD sub-ledger movements
/// (dividends, corporate-action fees, and the USD leg of each currency conversion).
/// The single-currency model can't represent those, so we import only EUR rows — the
/// EUR a dividend ultimately lands as still appears via its "Ingreso Cambio de Divisa"
/// row. Internal cash-sweep mirror legs ("Transferir … flatexDEGIRO Bank") carry no
/// amount and are skipped. There is no counterparty column, so the free-text
/// `Descripción` drives both the description and the classification concept, as in the
/// ING España importer.
fn parse_degiro(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    // The data table starts at the "Fecha,Hora,Fecha valor,…" header; DEGIRO exports
    // have no preamble, but locate it defensively like the other importers.
    let header_pos = text
        .lines()
        .position(|l| l.trim().starts_with("Fecha,Hora,Fecha valor"))
        .ok_or("Could not find the DEGIRO transaction table header")?;
    let data: String = text.lines().skip(header_pos).collect::<Vec<_>>().join("\n");

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(data.as_bytes());

    // The column layout is fixed and partly unnamed, so index positionally. The
    // amount's currency sits in `Variación` (7); the amount itself is the unnamed
    // field right after it (8).
    const DATE_I: usize = 0;
    const DESC_I: usize = 5;
    const CURRENCY_I: usize = 7;
    const AMOUNT_I: usize = 8;

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let line = n + 2;
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        // A row with no amount is an internal cash-sweep mirror leg ("Transferir …
        // flatexDEGIRO Bank"), not a transaction of its own. Skip it silently.
        let raw_amount = record.get(AMOUNT_I).map(str::trim).unwrap_or("");
        if raw_amount.is_empty() {
            continue;
        }

        // Only EUR rows are representable in the single-currency ledger; skip USD (and
        // any other) sub-ledger movements silently rather than mis-book them as euros.
        let currency = record.get(CURRENCY_I).map(str::trim).unwrap_or("");
        if !currency.eq_ignore_ascii_case("EUR") {
            continue;
        }

        let date = match normalize_date(record.get(DATE_I).unwrap_or("").trim()) {
            Ok(d) => d,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        let amount_cents = match parse_de_amount_cents(raw_amount) {
            Ok(a) => a,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let description = normalize_concept(record.get(DESC_I).unwrap_or("").trim());

        rows.push(ParsedRow {
            date,
            amount_cents,
            // No counterparty column; the free-text description is the best available
            // classification concept, mirroring the ING España importer.
            counterparty: description.clone(),
            description,
            category: None,
            // "ID Orden" is populated only for trades (empty for these cash
            // movements); fall back to date+amount+description for dedup.
            import_ref: String::new(),
        });
    }
    Ok((rows, errors))
}

// ----------------------------------------------------------------------------
// comdirect Girokonto
// ----------------------------------------------------------------------------

/// comdirect account "Umsätze" export: `;`-delimited with every field double-quoted,
/// German numbers (`-2.000,00`) and dates (`23.06.2026`), preceded by a metadata
/// preamble. Unlike ING, comdirect has no dedicated counterparty column — the payee is
/// embedded in the free-text `Buchungstext` as `Auftraggeber:`/`Empfänger: … Buchungstext:
/// …`. We lift it out via [`comdirect_counterparty`], falling back to the transaction
/// type (`Vorgang`) when no party is named (fees, coupons).
fn parse_comdirect(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    // The data table starts at the quoted `"Buchungstag";…` header; everything above
    // it is account metadata (banner, IBAN, balance).
    let header_pos = text
        .lines()
        .position(|l| l.trim().starts_with("\"Buchungstag\""))
        .ok_or("Could not find the comdirect transaction table header")?;
    let data: String = text.lines().skip(header_pos).collect::<Vec<_>>().join("\n");

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(data.as_bytes());

    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let idx = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let date_i = idx("Buchungstag").ok_or("comdirect CSV is missing the 'Buchungstag' column")?;
    let amount_i =
        idx("Umsatz in EUR").ok_or("comdirect CSV is missing the 'Umsatz in EUR' column")?;
    let vorgang_i = idx("Vorgang");
    let text_i = idx("Buchungstext");
    // The Visa export carries a per-transaction "Referenz" the Girokonto export
    // lacks; we use it as the duplicate key so repeated same-day, same-amount
    // charges at one merchant aren't collapsed into a single row.
    let ref_i = idx("Referenz");
    // Visa-Karte (credit-card) exports add an "Umsatztag" column and don't book
    // recent rows yet: those carry a non-date Buchungstag ("offen"/"neu") and are
    // split across two physical lines — a bare `"<date>";` line with no amount,
    // then the real row. Its presence flags the card dialect.
    let umsatztag_i = idx("Umsatztag");

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let line = n + 2;
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        // comdirect closes the table with an "Alter Kontostand" (opening balance)
        // trailer row; it isn't a transaction, so skip it rather than report it.
        if record.get(date_i).map(str::trim) == Some("Alter Kontostand") {
            continue;
        }

        // A row with no amount is not a transaction: either the Visa export's bare
        // `"<date>";` continuation line or a stray preamble line. Skip it silently.
        let raw_amount = record.get(amount_i).map(str::trim).unwrap_or("");
        if raw_amount.is_empty() {
            continue;
        }

        // Buchungstag holds a date for booked rows but the literal "offen"/"neu"
        // for not-yet-booked card rows; fall back to Umsatztag (the purchase date)
        // when it isn't a date.
        let raw_date = record.get(date_i).unwrap_or("").trim();
        let date_src = if normalize_date(raw_date).is_ok() {
            raw_date
        } else {
            umsatztag_i
                .and_then(|i| record.get(i))
                .map(str::trim)
                .unwrap_or(raw_date)
        };
        let date = match normalize_date(date_src) {
            Ok(d) => d,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        let amount_cents = match parse_de_amount_cents(raw_amount) {
            Ok(a) => a,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let buchungstext = record
            .get(text_i.unwrap_or(usize::MAX))
            .unwrap_or("")
            .trim();
        let vorgang = record
            .get(vorgang_i.unwrap_or(usize::MAX))
            .unwrap_or("")
            .trim();

        // On a card export the payee is the merchant in Buchungstext itself; the
        // Girokonto export instead embeds it behind Auftraggeber:/Empfänger: labels.
        let counterparty = if umsatztag_i.is_some() {
            let merchant = canonical_merchant(buchungstext);
            if merchant.is_empty() {
                normalize_concept(vorgang)
            } else {
                merchant
            }
        } else {
            comdirect_counterparty(buchungstext, vorgang)
        };

        let import_ref = ref_i
            .and_then(|i| record.get(i))
            .map(str::trim)
            .unwrap_or("")
            .to_string();

        rows.push(ParsedRow {
            date,
            amount_cents,
            counterparty,
            description: normalize_concept(buchungstext),
            category: None,
            import_ref,
        });
    }
    Ok((rows, errors))
}

/// Pull the counterparty out of a comdirect `Buchungstext`. Most entries lead with an
/// `Auftraggeber:` or `Empfänger:` label naming the party, followed by ` Buchungstext:`
/// and the free-text purpose — we take the text between the two. When no party is named
/// (account fees, security coupons), fall back to the transaction type (`Vorgang`) so
/// those rows still carry a stable classification key.
fn comdirect_counterparty(buchungstext: &str, vorgang: &str) -> String {
    for label in ["Auftraggeber:", "Empfänger:"] {
        if let Some(rest) = buchungstext.strip_prefix(label) {
            let party = match rest.find("Buchungstext:") {
                Some(pos) => &rest[..pos],
                None => rest,
            };
            return normalize_concept(party);
        }
    }
    normalize_concept(vorgang)
}

// ----------------------------------------------------------------------------
// Deutsche Bank / maxblue
// ----------------------------------------------------------------------------

/// Deutsche Bank (and maxblue) account "Umsätze" export: `;`-delimited, unquoted,
/// German numbers (`-134,87`) and dates (`15.6.2026`, unpadded), preceded by a
/// metadata preamble (banner, IBAN, opening balance) and closed by a `Kontostand`
/// balance trailer. The `Betrag` column already carries the sign, so we read it
/// directly rather than the split `Soll`/`Haben` columns. The counterparty comes
/// from `Begünstigter / Auftraggeber`; depot bookings (dividends, fees) leave that
/// empty, so we fall back to the transaction type (`Umsatzart`) for a stable
/// classification key, mirroring the comdirect `Vorgang` fallback.
fn parse_db(text: &str) -> Result<(Vec<ParsedRow>, Vec<String>), String> {
    // The data table starts at the "Buchungstag;Wert;Umsatzart;…" header; everything
    // above it is account metadata (banner, IBAN, opening balance).
    let header_pos = text
        .lines()
        .position(|l| l.trim().starts_with("Buchungstag;Wert;Umsatzart"))
        .ok_or("Could not find the Deutsche Bank transaction table header")?;
    let data: String = text.lines().skip(header_pos).collect::<Vec<_>>().join("\n");

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(data.as_bytes());

    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let idx = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let date_i =
        idx("Buchungstag").ok_or("Deutsche Bank CSV is missing the 'Buchungstag' column")?;
    let amount_i = idx("Betrag").ok_or("Deutsche Bank CSV is missing the 'Betrag' column")?;
    let payee_i = idx("Begünstigter / Auftraggeber");
    let type_i = idx("Umsatzart");
    let purpose_i = idx("Verwendungszweck");

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let line = n + 2;
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        // Deutsche Bank closes the table with a "Kontostand" (closing balance)
        // trailer row; it isn't a transaction, so skip it rather than report it.
        if record.get(date_i).map(str::trim) == Some("Kontostand") {
            continue;
        }

        let date = match normalize_date(record.get(date_i).unwrap_or("").trim()) {
            Ok(d) => d,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };
        let amount_cents = match parse_de_amount_cents(record.get(amount_i).unwrap_or("").trim()) {
            Ok(a) => a,
            Err(err) => {
                errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let payee = payee_i.and_then(|i| record.get(i)).unwrap_or("").trim();
        let counterparty = if payee.is_empty() {
            normalize_concept(type_i.and_then(|i| record.get(i)).unwrap_or("").trim())
        } else {
            normalize_concept(payee)
        };
        let description =
            normalize_concept(purpose_i.and_then(|i| record.get(i)).unwrap_or("").trim());

        rows.push(ParsedRow {
            date,
            amount_cents,
            counterparty,
            description,
            category: None,
            // Deutsche Bank's "Kundenreferenz" is NOT a reliable per-transaction
            // identity — it's routinely a shared placeholder ("NOTPROVIDED"/"NONREF")
            // or a batch reference repeated across unrelated rows, so using it as the
            // dedup key collapses genuinely distinct transactions. Leave it empty and
            // fall back to date+amount+description, as the ING importer does.
            import_ref: String::new(),
        });
    }
    Ok((rows, errors))
}

// ----------------------------------------------------------------------------
// Shared parsing helpers
// ----------------------------------------------------------------------------

/// Collapse a counterparty string into a stable "concept" key: trimmed with runs
/// of whitespace reduced to single spaces. Used both as the value stored on a
/// transaction and as the classification-rule key.
pub(crate) fn normalize_concept(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Turn a card merchant string into a stable classification concept.
///
/// Card exports tack a per-purchase order/authorization token onto the merchant
/// (e.g. `AMZN Mktp DE H110A8TU5`), and one payee can appear under several brand
/// strings. Left alone, every purchase gets a unique concept and a learned
/// classification rule never generalizes. We fold the known multi-name brands to a
/// single label so one rule covers them all; everything else is just whitespace-
/// normalized (its full text preserved, since store numbers like `Rossmann 298`
/// are stable and meaningful).
pub(crate) fn canonical_merchant(raw: &str) -> String {
    let concept = normalize_concept(raw);
    let lower = concept.to_lowercase();
    // Amazon bills as "AMZN Mktp DE …", "Amazon.de …", "AMAZON PRIM …",
    // "WWW.AMAZON. …", "AMAZON …", "AMZN.COM/BILL" — all the same payee here.
    if lower.contains("amazon") || lower.contains("amzn") {
        return "Amazon".to_string();
    }
    concept
}

pub(crate) fn validate_date(date: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| format!("Invalid date \"{date}\" (expected YYYY-MM-DD)"))
}

/// Accept a few common date layouts and normalize to YYYY-MM-DD.
///
/// Slash- and dot-separated dates are read day-first (`DD/MM/YYYY`), matching the
/// European banks this app imports and what the Import UI documents. We deliberately
/// do NOT also try a month-first (`MM/DD/YYYY`) layout: mixing the two lets one file
/// be interpreted inconsistently row by row — `13/04` as day-first but `04/13` as
/// month-first — silently corrupting dates for days ≤ 12. A genuinely month-first
/// date (`04/13/2026`) instead fails here and is surfaced as a bad row.
pub(crate) fn normalize_date(raw: &str) -> Result<String, String> {
    for fmt in [
        "%Y-%m-%d", "%d.%m.%Y", // German: 22.06.2026 (also parses 1.1.2026)
        "%d/%m/%Y", // day-first; see the doc comment on why MM/DD is excluded
        "%Y/%m/%d", "%d-%m-%Y",
    ] {
        if let Ok(d) = NaiveDate::parse_from_str(raw, fmt) {
            return Ok(d.format("%Y-%m-%d").to_string());
        }
    }
    Err(format!("Invalid date \"{raw}\" (expected YYYY-MM-DD)"))
}

/// Normalize an ING España date, which mixes full `DD/MM/YYYY` (`14/04/2026`) with a
/// short `D/M/YY` (`4/6/26`) in the same file. We can't route these through
/// [`normalize_date`]: its `%d/%m/%Y` pass would happily read the 2-digit "26" as the
/// year 26 AD. Instead we pick the year width from the third field so "26" is
/// interpreted as 2026.
fn normalize_es_date(raw: &str) -> Result<String, String> {
    let fmt = match raw.rsplit('/').next() {
        Some(year) if year.len() <= 2 => "%d/%m/%y",
        _ => "%d/%m/%Y",
    };
    NaiveDate::parse_from_str(raw, fmt)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| format!("Invalid date \"{raw}\" (expected DD/MM/YYYY)"))
}

/// Parse a decimal amount that uses a `.` decimal separator (the canonical format)
/// into integer cents.
pub(crate) fn parse_amount_cents(raw: &str) -> Result<i64, String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    decimal_str_to_cents(&cleaned).ok_or_else(|| format!("Invalid amount \"{raw}\""))
}

/// Parse a German-formatted amount (`.` thousands, `,` decimals — e.g. `-5.000,00`)
/// into integer cents.
pub(crate) fn parse_de_amount_cents(raw: &str) -> Result<i64, String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.' || *c == ',')
        .collect();
    // Thousands dots drop out; the decimal comma becomes a point.
    let normalized = cleaned.replace('.', "").replace(',', ".");
    decimal_str_to_cents(&normalized).ok_or_else(|| format!("Invalid amount \"{raw}\""))
}

/// Convert a cleaned decimal string (digits, an optional leading `-`, and at most
/// one `.`) into integer cents, rounding half-up on the third decimal. Returns
/// `None` for anything that is not a well-formed number.
///
/// Mirrors `parseAmountToCents` in the front end (`src/format.ts`); keep in sync.
pub(crate) fn decimal_str_to_cents(s: &str) -> Option<i64> {
    let negative = s.starts_with('-');
    let body = s.strip_prefix('-').unwrap_or(s);
    if body.is_empty() {
        return None;
    }

    let mut parts = body.splitn(2, '.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next().unwrap_or("");

    // A second '.' (e.g. "1.2.3") lands inside frac_part; reject it.
    if frac_part.contains('.') {
        return None;
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }

    let int_val: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };

    let digit = |i: usize| frac_part.as_bytes().get(i).map_or(0, |b| (b - b'0') as i64);
    let mut cents = int_val.checked_mul(100)? + digit(0) * 10 + digit(1);
    if digit(2) >= 5 {
        cents += 1;
    }

    Some(if negative { -cents } else { cents })
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- amount parsing ------------------------------------------------------

    #[test]
    fn parses_plain_decimals_without_float_error() {
        assert_eq!(parse_amount_cents("12.34").unwrap(), 1234);
        assert_eq!(parse_amount_cents("-12.34").unwrap(), -1234);
        assert_eq!(parse_amount_cents("1500.00").unwrap(), 150000);
        assert_eq!(parse_amount_cents("0.01").unwrap(), 1);
        assert_eq!(parse_amount_cents("0").unwrap(), 0);
        assert_eq!(parse_amount_cents("0.29").unwrap(), 29);
    }

    #[test]
    fn parses_partial_and_shorthand_decimals() {
        assert_eq!(parse_amount_cents("5").unwrap(), 500);
        assert_eq!(parse_amount_cents("5.5").unwrap(), 550);
        assert_eq!(parse_amount_cents(".5").unwrap(), 50);
        assert_eq!(parse_amount_cents("-.5").unwrap(), -50);
    }

    #[test]
    fn strips_currency_symbols_and_thousands_separators() {
        assert_eq!(parse_amount_cents("$1234.56").unwrap(), 123456);
        assert_eq!(parse_amount_cents("€ 42.90").unwrap(), 4290);
    }

    #[test]
    fn rounds_half_up_on_the_third_decimal() {
        assert_eq!(parse_amount_cents("12.345").unwrap(), 1235);
        assert_eq!(parse_amount_cents("12.344").unwrap(), 1234);
        assert_eq!(parse_amount_cents("-12.345").unwrap(), -1235);
    }

    #[test]
    fn rejects_malformed_amounts() {
        for bad in ["", "-", "abc", "1.2.3", ".", "--5"] {
            assert!(parse_amount_cents(bad).is_err(), "expected {bad:?} to fail");
        }
    }

    #[test]
    fn parses_german_amounts() {
        // Dot = thousands, comma = decimals.
        assert_eq!(parse_de_amount_cents("-5.000,00").unwrap(), -500000);
        assert_eq!(parse_de_amount_cents("5.082,40").unwrap(), 508240);
        assert_eq!(parse_de_amount_cents("-1,49").unwrap(), -149);
        assert_eq!(parse_de_amount_cents("518,00").unwrap(), 51800);
        assert_eq!(parse_de_amount_cents("-2.000,00").unwrap(), -200000);
        // A bare integer with a thousands dot and no decimals.
        assert_eq!(parse_de_amount_cents("1.234").unwrap(), 123400);
    }

    // --- date parsing --------------------------------------------------------

    #[test]
    fn normalizes_common_date_layouts() {
        assert_eq!(normalize_date("2026-01-05").unwrap(), "2026-01-05");
        assert_eq!(normalize_date("05/01/2026").unwrap(), "2026-01-05"); // DD/MM/YYYY
        assert_eq!(normalize_date("2026/01/05").unwrap(), "2026-01-05");
        assert_eq!(normalize_date("22.06.2026").unwrap(), "2026-06-22"); // German
        assert_eq!(normalize_date("1.1.2026").unwrap(), "2026-01-01"); // unpadded
    }

    #[test]
    fn slash_dates_are_day_first_and_month_first_is_rejected() {
        // Ambiguous slash dates are always read day-first, never month-first, so a
        // file can't be interpreted inconsistently row to row.
        assert_eq!(normalize_date("03/04/2026").unwrap(), "2026-04-03"); // 3 April, not 4 March
                                                                         // A genuinely month-first date is a bad row, not a silent misparse: "13" is
                                                                         // not a valid month under the day-first layout, so it fails loudly.
        assert!(normalize_date("04/13/2026").is_err());
    }

    #[test]
    fn rejects_invalid_dates() {
        assert!(normalize_date("not-a-date").is_err());
        assert!(validate_date("2026-13-40").is_err());
        assert!(validate_date("2026-02-15").is_ok());
    }

    // --- concept normalization ----------------------------------------------

    #[test]
    fn normalize_concept_collapses_whitespace() {
        assert_eq!(
            normalize_concept("  BMW   Car  IT GmbH "),
            "BMW Car IT GmbH"
        );
        assert_eq!(normalize_concept(""), "");
    }

    // --- format detection ----------------------------------------------------

    #[test]
    fn detects_ing_and_canonical() {
        let ing = "Umsatzanzeige;Datei erstellt am: 26.06.2026\n\n\
                   Buchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;\
                   Verwendungszweck;Saldo;Währung;Betrag;Währung\n";
        assert_eq!(detect_format(ing), BankFormat::IngDiba);
        assert_eq!(
            detect_format("date,amount,description\n2026-01-01,-1.00,x\n"),
            BankFormat::Canonical
        );
    }

    #[test]
    fn format_labels_are_human_readable() {
        assert_eq!(BankFormat::IngDiba.label(), "ING-DiBa");
        assert_eq!(BankFormat::Canonical.label(), "Canonical template");
    }

    // --- ING parsing ---------------------------------------------------------

    const ING_SAMPLE: &str = "Umsatzanzeige;Datei erstellt am: 26.06.2026 14:43\n\
\n\
IBAN;DE80 5001 0517 5410 8018 30\n\
Kontoname;Girokonto\n\
Bank;ING\n\
Saldo;26.021,35;EUR\n\
\n\
Buchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;Verwendungszweck;Saldo;Währung;Betrag;Währung\n\
22.06.2026;20.06.2026;Ismael Jimenez Martinez;Echtzeitüberweisung;;26.021,35;EUR;-5.000,00;EUR\n\
19.06.2026;19.06.2026;BMW Car IT GmbH;Gehalt/Rente;Verdienstabrechnung 06.26/1;31.021,35;EUR;5.082,40;EUR\n";

    #[test]
    fn parses_ing_export() {
        let (rows, errors) = parse(ING_SAMPLE).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows.len(), 2);

        assert_eq!(
            rows[0],
            ParsedRow {
                date: "2026-06-22".into(),
                amount_cents: -500000,
                counterparty: "Ismael Jimenez Martinez".into(),
                description: "Echtzeitüberweisung".into(),
                category: None,
                import_ref: String::new(),
            }
        );
        assert_eq!(rows[1].counterparty, "BMW Car IT GmbH");
        assert_eq!(rows[1].amount_cents, 508240);
        // Buchungstext + Verwendungszweck are joined.
        assert_eq!(
            rows[1].description,
            "Gehalt/Rente Verdienstabrechnung 06.26/1"
        );
    }

    #[test]
    fn ing_without_header_is_a_fatal_error() {
        // Trips detection via the banner but has no data header.
        let err = parse("Umsatzanzeige;x\n\nnonsense\n").unwrap_err();
        assert!(err.contains("header"), "unexpected error: {err}");
    }

    // --- ING España parsing --------------------------------------------------

    // A Spanish ING export: a UTF-8 BOM on the banner, a metadata preamble, the
    // `,`-delimited `F. VALOR,…` data header, US-style amounts, and dates that mix
    // full `DD/MM/YYYY` with the short `D/M/YY`. The SALDO balance column carries
    // quoted thousands separators we ignore.
    const ING_ES_SAMPLE: &str = "\u{feff}Movimientos de la Cuenta,,  Número de cuenta:,1465 0100 9617 08810024,,,\n\
,,  Titular:,ISMAEL JIMENEZ MARTINEZ,,,\n\
,,  Fecha exportación:,05/07/2026 10:55h,,,\n\
F. VALOR,CATEGORÍA,SUBCATEGORÍA,DESCRIPCIÓN,COMENTARIO,IMPORTE (€),SALDO (€)\n\
4/6/26,Otros gastos,Comisiones e intereses,Comisión de mantenimiento de cuenta,,-3,\"1,158.89\"\n\
14/04/2026,Otros gastos,Comisiones e intereses,Comisión de custodia,,-14.52,\"1,164.89\"\n\
7/4/26,Inversión,Acciones,Abono por evento DIVIDENDO (VANGUARD S&P 500 UCITS ETF),,28.6,\"1,179.41\"\n";

    #[test]
    fn detects_ing_es() {
        assert_eq!(detect_format(ING_ES_SAMPLE), BankFormat::IngEs);
    }

    #[test]
    fn parses_ing_es_export() {
        let (rows, errors) = parse(ING_ES_SAMPLE).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows.len(), 3);

        // Short D/M/YY date; US-style negative amount; DESCRIPCIÓN drives both the
        // description and the classification concept. Category is left unset.
        assert_eq!(
            rows[0],
            ParsedRow {
                date: "2026-06-04".into(),
                amount_cents: -300,
                counterparty: "Comisión de mantenimiento de cuenta".into(),
                description: "Comisión de mantenimiento de cuenta".into(),
                category: None,
                import_ref: String::new(),
            }
        );

        // Full DD/MM/YYYY date parses too.
        assert_eq!(rows[1].date, "2026-04-14");
        assert_eq!(rows[1].amount_cents, -1452);

        // A positive amount (dividend) is income; fractional dot decimal handled.
        assert_eq!(rows[2].date, "2026-04-07");
        assert_eq!(rows[2].amount_cents, 2860);
        assert_eq!(
            rows[2].counterparty,
            "Abono por evento DIVIDENDO (VANGUARD S&P 500 UCITS ETF)"
        );
    }

    #[test]
    fn ing_es_short_and_full_years() {
        assert_eq!(normalize_es_date("4/6/26").unwrap(), "2026-06-04");
        assert_eq!(normalize_es_date("14/04/2026").unwrap(), "2026-04-14");
        assert_eq!(normalize_es_date("7/4/26").unwrap(), "2026-04-07");
        assert!(normalize_es_date("not-a-date").is_err());
    }

    // --- DEGIRO parsing ------------------------------------------------------

    // A DEGIRO Spanish cash export: no preamble, a `,`-delimited header whose amount
    // and balance columns are unnamed, `DD-MM-YYYY` dates, quoted comma-decimal
    // amounts, an empty-amount "Transferir" mirror leg, and USD sub-ledger rows
    // (dividend, corporate-action fee, FX withdrawal leg) that must be skipped.
    const DEGIRO_SAMPLE: &str = "\
Fecha,Hora,Fecha valor,Producto,ISIN,Descripción,Tipo,Variación,,Saldo,,ID Orden\n\
05-04-2026,22:30,31-03-2026,,,Flatex Interest Income,,EUR,\"0,00\",EUR,\"468,04\",\n\
03-04-2026,10:48,03-04-2026,,,Degiro Cash Sweep Transfer,,EUR,\"-51,18\",EUR,\"468,04\",\n\
03-04-2026,10:48,03-04-2026,,,\"Transferir a su Cuenta de Efectivo en flatexDEGIRO Bank: 51,18 EUR\",,,,EUR,\"519,22\",\n\
03-04-2026,07:18,02-04-2026,,,Ingreso Cambio de Divisa,,EUR,\"51,18\",EUR,\"468,04\",\n\
03-04-2026,07:18,02-04-2026,,,Retirada Cambio de Divisa,\"1,1568\",USD,\"-59,21\",USD,\"0,00\",\n\
02-04-2026,07:41,01-04-2026,VANGUARD FTSE ALL-WORLD UCITS ETF,IE00B3RBWM25,Dividendo,,USD,\"62,24\",USD,\"62,24\",\n";

    #[test]
    fn detects_degiro() {
        assert_eq!(detect_format(DEGIRO_SAMPLE), BankFormat::Degiro);
    }

    #[test]
    fn parses_degiro_export() {
        let (rows, errors) = parse(DEGIRO_SAMPLE).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        // Only the three EUR rows survive: the two USD rows and the empty-amount
        // "Transferir" mirror leg are skipped (not reported as errors).
        assert_eq!(rows.len(), 3);

        // Positional amount (field after Variación); DD-MM-YYYY date; comma decimal.
        // Descripción drives both description and the classification concept.
        assert_eq!(
            rows[0],
            ParsedRow {
                date: "2026-04-05".into(),
                amount_cents: 0,
                counterparty: "Flatex Interest Income".into(),
                description: "Flatex Interest Income".into(),
                category: None,
                import_ref: String::new(),
            }
        );

        // A negative cash-sweep amount is an expense.
        assert_eq!(rows[1].date, "2026-04-03");
        assert_eq!(rows[1].amount_cents, -5118);
        assert_eq!(rows[1].counterparty, "Degiro Cash Sweep Transfer");

        // The EUR conversion inflow is kept (this is the euros a USD dividend lands as).
        assert_eq!(rows[2].counterparty, "Ingreso Cambio de Divisa");
        assert_eq!(rows[2].amount_cents, 5118);

        // The USD "Dividendo"/"Retirada" rows are never imported.
        assert!(
            rows.iter()
                .all(|r| r.counterparty != "Dividendo"
                    && r.counterparty != "Retirada Cambio de Divisa"),
            "USD rows must not be imported"
        );
    }

    #[test]
    fn degiro_without_header_is_a_fatal_error() {
        let err = parse_degiro("nonsense\nno table here\n").unwrap_err();
        assert!(err.contains("header"), "unexpected error: {err}");
    }

    // --- comdirect parsing ---------------------------------------------------

    const COMDIRECT_SAMPLE: &str = "\
;\n\
\"Umsätze Girokonto\";\"Zeitraum: 01.01.2026 - 25.06.2026\";\n\
\"Neuer Kontostand\";\"13.965,56 EUR\";\n\
\n\
\"Buchungstag\";\"Wertstellung (Valuta)\";\"Vorgang\";\"Buchungstext\";\"Umsatz in EUR\";\n\
\"23.06.2026\";\"23.06.2026\";\"Lastschrift / Belastung\";\"Auftraggeber: PayPal Europe S.a.r.l. et Cie S.C.A Buchungstext: 1051100334377/PP.1929.PP/. DocMorris NV Ref. 5A2C296Z42OT5OZR/5743\";\"-43,94\";\n\
\"22.06.2026\";\"22.06.2026\";\"Übertrag / Überweisung\";\"Empfänger: Ismael Jimenez Martinez Buchungstext: 5510000496491140-693733 Ueberweisung Ref. 042C297208NUL22Q/74265\";\"-2.000,00\";\n\
\"01.06.2026\";\"29.05.2026\";\"Kontoführungsentgelt\";\" Buchungstext: Entgelt Visa-Kreditkarte Zeitraum: 01.05.2026 bis 31.05.2026 Ref. 852C296H2WA36TWP/640594\";\"-1,90\";\n\
\n\
\"Alter Kontostand\";\"12.606,89 EUR\";\n";

    #[test]
    fn detects_comdirect() {
        assert_eq!(detect_format(COMDIRECT_SAMPLE), BankFormat::Comdirect);
    }

    #[test]
    fn parses_comdirect_export() {
        let (rows, errors) = parse(COMDIRECT_SAMPLE).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows.len(), 3);

        // Auftraggeber is lifted out as the counterparty; German date/amount parsed.
        assert_eq!(rows[0].date, "2026-06-23");
        assert_eq!(rows[0].amount_cents, -4394);
        assert_eq!(rows[0].counterparty, "PayPal Europe S.a.r.l. et Cie S.C.A");

        // Empfänger works the same way; thousands separator handled.
        assert_eq!(rows[1].counterparty, "Ismael Jimenez Martinez");
        assert_eq!(rows[1].amount_cents, -200000);

        // No party named → fall back to the Vorgang (transaction type).
        assert_eq!(rows[2].counterparty, "Kontoführungsentgelt");
        assert_eq!(rows[2].amount_cents, -190);
    }

    #[test]
    fn comdirect_counterparty_extraction() {
        assert_eq!(
            comdirect_counterparty("Auftraggeber: ACME GmbH Buchungstext: rent", "x"),
            "ACME GmbH"
        );
        assert_eq!(
            comdirect_counterparty("Empfänger: Jane Doe Buchungstext: gift", "x"),
            "Jane Doe"
        );
        // No "Buchungstext:" marker: take the whole remainder.
        assert_eq!(
            comdirect_counterparty("Auftraggeber: Solo Party", "x"),
            "Solo Party"
        );
        // No party label at all: fall back to Vorgang.
        assert_eq!(
            comdirect_counterparty("Buchungstext: Entgelt", "Kupon"),
            "Kupon"
        );
    }

    #[test]
    fn comdirect_skips_the_balance_trailer() {
        // The "Alter Kontostand" trailer must not surface as a bad row.
        let (rows, errors) = parse(COMDIRECT_SAMPLE).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn comdirect_without_header_is_a_fatal_error() {
        let err = parse("\"Umsätze Girokonto\";x\n\nnonsense\n").unwrap_err();
        assert!(err.contains("header"), "unexpected error: {err}");
    }

    // The Visa-Karte export differs from the Girokonto one: an extra "Umsatztag"
    // column, a "Referenz" column, a non-date Buchungstag ("offen"/"neu") on
    // unbooked rows, and each unbooked row split across a bare `"<date>";` line
    // plus the real row. The merchant lives directly in Buchungstext.
    const COMDIRECT_VISA_SAMPLE: &str = "\
;\n\
\"Umsätze Visa-Karte (Kreditkarte) ..9255 (Ismael)\";\"Zeitraum: 01.01.2026 - 25.06.2026\";\n\
\"Neuer Kontostand\";\"2.710,34 EUR\";\n\
\n\
\"Buchungstag\";\"Umsatztag\";\"Vorgang\";\"Referenz\";\"Buchungstext\";\"Umsatz in EUR\";\n\
\"offen\";\"24.06.2026\";\"Kartenumsatz\";\"151420018103\";\" ALDI NORD// BREMERHAVEN/ DEU \";\"-82,03\";\n\
\"24.06.2026\";\"23.06.2026\";\"Kartenumsatz\";\"151150884903\";\" STADTBAECKEREI ENGELBR \";\"-7,90\";\n\
\"22.06.2026\";\"22.06.2026\";\"Uebertrag auf Visa-Karte\";\"150528448003\";\" Uebertrag auf Visa-Karte \";\"2.000,00\";\n\
\"13.01.2026\";\n\
\"neu\";\"12.01.2026\";\"Kartenumsatz\";\"110092508203\";\" Amazon.de Z73G55TA4 \";\"-5,95\";\n\
\n\
\"Alter Kontostand\";\"1.958,12 EUR\";\n";

    #[test]
    fn detects_comdirect_visa() {
        assert_eq!(detect_format(COMDIRECT_VISA_SAMPLE), BankFormat::Comdirect);
    }

    #[test]
    fn parses_comdirect_visa_export() {
        let (rows, errors) = parse(COMDIRECT_VISA_SAMPLE).unwrap();
        // The bare `"13.01.2026";` continuation line and the balance trailer must
        // not surface as bad rows.
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows.len(), 4);

        // Unbooked ("offen") row: Buchungstag isn't a date, so fall back to
        // Umsatztag. Merchant lifted straight from Buchungstext.
        assert_eq!(rows[0].date, "2026-06-24");
        assert_eq!(rows[0].amount_cents, -8203);
        assert_eq!(rows[0].counterparty, "ALDI NORD// BREMERHAVEN/ DEU");
        assert_eq!(rows[0].description, "ALDI NORD// BREMERHAVEN/ DEU");
        // The Referenz column is captured as the duplicate-detection key.
        assert_eq!(rows[0].import_ref, "151420018103");

        // Booked row uses the (date) Buchungstag directly.
        assert_eq!(rows[1].date, "2026-06-24");
        assert_eq!(rows[1].counterparty, "STADTBAECKEREI ENGELBR");

        // A positive top-up (Uebertrag) is income; falls back through Buchungstext.
        assert_eq!(rows[2].amount_cents, 200000);

        // "neu" row: date comes from Umsatztag (12.01), not the "neu" Buchungstag.
        assert_eq!(rows[3].date, "2026-01-12");
        // The Amazon merchant folds to a stable concept for classification, while
        // the description keeps the full original text (order token and all).
        assert_eq!(rows[3].counterparty, "Amazon");
        assert_eq!(rows[3].description, "Amazon.de Z73G55TA4");
    }

    #[test]
    fn canonical_merchant_folds_amazon_variants() {
        // Every Amazon brand string + per-order token collapses to one concept.
        for raw in [
            " AMZN Mktp DE XC4KJ1U65 ",
            " Amazon.de O59H84AO5 ",
            " AMAZON PRIM GM2182O25 ",
            " WWW.AMAZON. NR94D9LQ4 ",
            " AMAZON 0U63D7LA5 ",
            " AMZN Mktp DE// AMZN.COM/BILL/ LUX ",
        ] {
            assert_eq!(canonical_merchant(raw), "Amazon", "for {raw:?}");
        }
        // Non-Amazon merchants are only whitespace-normalized; stable store numbers
        // and other tokens are preserved so distinct shops stay distinct.
        assert_eq!(canonical_merchant("  Rossmann   298 "), "Rossmann 298");
        assert_eq!(
            canonical_merchant(" STADTBAECKEREI ENGELBR "),
            "STADTBAECKEREI ENGELBR"
        );
    }

    // --- Deutsche Bank parsing -----------------------------------------------

    // A maxblue depot export: metadata preamble, an opening "Letzter Kontostand"
    // line, the unquoted `Buchungstag;Wert;Umsatzart;…` header, booked rows whose
    // signed amount lives in `Betrag`, and a closing "Kontostand" balance trailer.
    // Depot bookings leave the counterparty column empty.
    const DB_SAMPLE: &str = "\
Umsätze\n\
Konto;Filial-/Kontonummer;IBAN;Währung\n\
maxblue Depotkonto;220 7810237 01;DE93700700240781023701;EUR\n\
\n\
1.1.2026 - 26.6.2026\n\
Letzter Kontostand;;;;2.137,38;EUR\n\
Vorgemerkte und noch nicht gebuchte Umsätze sind nicht Bestandteil dieser Übersicht.\n\
Buchungstag;Wert;Umsatzart;Begünstigter / Auftraggeber;Verwendungszweck;IBAN / Kontonummer;BIC;Kundenreferenz;Mandatsreferenz;Gläubiger ID;Fremde Gebühren;Betrag;Abweichender Empfänger;Anzahl der Aufträge;Anzahl der Schecks;Soll;Haben;Währung\n\
15.6.2026;15.6.2026;Wertpapiere;;INTEREST/DIVIDEND/EARNINGS QTY/NOM: 325 ISH.ST.EURO.SMALL 200 U.ETF DE INH.ANT .;;;;;;;120,09;;;;;120,09;EUR\n\
22.1.2026;2.1.2026;Zinsen/Dividenden/Erträge;;ADVANCED LUMP SUM QTY/NOM: 1244 IS C.MSCI EMIMI U.ETF DLA FUNDS;;;;;;;-134,87;;;;-134,87;;EUR\n\
Kontostand;26.6.2026;;;2.740,81;EUR\n";

    #[test]
    fn detects_deutsche_bank() {
        assert_eq!(detect_format(DB_SAMPLE), BankFormat::DeutscheBank);
    }

    #[test]
    fn parses_deutsche_bank_export() {
        let (rows, errors) = parse(DB_SAMPLE).unwrap();
        // The opening/closing balance lines must not surface as bad rows.
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(rows.len(), 2);

        // Unpadded German date; signed amount read straight from Betrag (Haben).
        // No counterparty named → fall back to the Umsatzart transaction type.
        assert_eq!(rows[0].date, "2026-06-15");
        assert_eq!(rows[0].amount_cents, 12009);
        assert_eq!(rows[0].counterparty, "Wertpapiere");
        assert!(
            rows[0]
                .description
                .starts_with("INTEREST/DIVIDEND/EARNINGS"),
            "description was {:?}",
            rows[0].description
        );

        // Negative Betrag (Soll) is an expense; Umsatzart fallback again.
        assert_eq!(rows[1].date, "2026-01-22");
        assert_eq!(rows[1].amount_cents, -13487);
        assert_eq!(rows[1].counterparty, "Zinsen/Dividenden/Erträge");
    }

    #[test]
    fn deutsche_bank_without_header_is_a_fatal_error() {
        // A file routed to the DB parser but missing the data header is a whole-file
        // failure, not a silent zero-row parse.
        let err = parse_db("Umsätze\nno transaction table here\n").unwrap_err();
        assert!(err.contains("header"), "unexpected error: {err}");
    }

    // --- canonical parsing ---------------------------------------------------

    #[test]
    fn parses_canonical_with_category() {
        let csv = "date,amount,description,category\n\
                   2026-01-05,-42.90,Grocery store,Groceries\n";
        let (rows, errors) = parse(csv).unwrap();
        assert!(errors.is_empty());
        assert_eq!(
            rows[0],
            ParsedRow {
                date: "2026-01-05".into(),
                amount_cents: -4290,
                counterparty: String::new(),
                description: "Grocery store".into(),
                category: Some("Groceries".into()),
                import_ref: String::new(),
            }
        );
    }

    #[test]
    fn canonical_requires_expected_columns() {
        let err = parse("foo,bar\n1,2\n").unwrap_err();
        assert!(err.contains("date"), "unexpected error: {err}");
    }

    #[test]
    fn canonical_reports_bad_rows_but_keeps_going() {
        let csv = "date,amount,description\n\
                   not-a-date,-1.00,Bad date\n\
                   2026-01-06,oops,Bad amount\n\
                   2026-01-07,-9.99,Good row\n";
        let (rows, errors) = parse(csv).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(errors.len(), 2);
    }
}
