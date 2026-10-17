# BRP-koppeling

Dit document beschrijft hoe e-KS gegevens uit de Basisregistratie Personen
(BRP) raadpleegt om de kandidaten op een ingeleverde kandidatenlijst te
controleren: wanneer wordt bevraagd, welke gegevens worden verstuurd en
ontvangen, wat daarvan wordt vastgelegd, en welke fouten in welke situaties
ontstaan.

e-KS heeft geen directe verbinding met de BRP. Alle bevragingen lopen via een
tussenpartij die de BRP-gegevens ontsluit en die e-KS met een API-sleutel
aanspreekt. Waar dit document "de BRP" noemt, gaat het om de gegevens uit de
BRP zoals de tussenpartij die teruggeeft; waar het om de verbinding, de
beschikbaarheid of een foutmelding gaat, is de tussenpartij bedoeld.

Het is geschreven voor wie de werking van de koppeling wil begrijpen of
toetsen. Voor de traceerbaarheid: de
koppeling is geïmplementeerd in `src/structs/brp/` (de aanroep, de verstuurde
en ontvangen gegevens, de bevindingen) en `src/csb/import/pages/import.rs` (de
controleronde). Waar dit document een technische naam noemt, is dat de naam
zoals die in de code en het auditlog voorkomt.

## 1. Samenvatting

- Alleen het centraal stembureau (CSB) raadpleegt de BRP. Een politieke
  groepering kan dat vanuit e-KS niet.
- e-KS bevraagt de BRP niet rechtstreeks, maar via een tussenpartij. Die
  biedt de landelijke API *BRP Personen bevragen* van Haal Centraal (versie 2)
  aan en geeft e-KS daarvoor een API-sleutel.
- Per kandidaat vraagt e-KS een vaste, beperkte set gegevens op: naam,
  geslacht, geboortedatum, woonplaats, overlijden, nationaliteit, uitsluiting
  van het kiesrecht en de achternamen van (ex-)partners. Niets anders. Deze
  set is toegesneden op de Eerste Kamerverkiezing (en de Tweede
  Kamerverkiezing); andere verkiezingen vragen later om meer, zie 4.1.
- Wat de BRP over een kandidaat zegt, wordt een **bevinding** bij die
  kandidaat. Het wordt nooit automatisch een verzuim: het CSB beoordeelt elke
  bevinding zelf.
- Elke bevraging wordt vastgelegd in het auditlog van de betreffende lijst,
  ook als de bevraging mislukt.
- Als de tussenpartij niet bereikbaar is of een fout teruggeeft, stopt de
  controle en wordt dat als zodanig vastgelegd. Een mislukte bevraging wordt
  nooit geïnterpreteerd als "geen fouten gevonden".

## 2. De verbinding

e-KS maakt verbinding met de tussenpartij, niet met de BRP zelf. De
tussenpartij stelt de API *BRP Personen bevragen* van Haal Centraal
beschikbaar; e-KS spreekt die API aan in de vorm die de landelijke specificatie
voorschrijft.

| Onderdeel | Waarde |
| --- | --- |
| API | Haal Centraal BRP Personen bevragen, versie 2, aangeboden door de tussenpartij |
| Aanroep | `POST {BRP_BASE_URL}/{BRP_PERSONS_ENDPOINT}`, waarbij `BRP_BASE_URL` het adres van de tussenpartij is |
| Standaardpad | `haalcentraal/api/brp/personen` |
| Authenticatie | HTTP-header `Authorization: Bearer {BRP_API_KEY}`, met de sleutel die de tussenpartij aan e-KS heeft uitgegeven |
| Inhoud | JSON, één van de twee bevragingstypen uit hoofdstuk 4 |
| Time-out | `BRP_TIMEOUT` seconden per bevraging, standaard 30 |
| Herhaalpogingen | Geen |
| Lokale ontwikkeling | De container `personen-mock` uit `compose.yml`, een door de beheerders van de Haal Centraal-API gepubliceerde nabootsing die dezelfde API aanbiedt als de tussenpartij |

De instellingen komen uit omgevingsvariabelen. `BRP_BASE_URL` en `BRP_API_KEY`
zijn verplicht; `BRP_PERSONS_ENDPOINT` en `BRP_TIMEOUT` zijn optioneel. De
API-sleutel wordt in het geheugen als geheim behandeld, zodat die niet in een
logregel terecht kan komen.

Elke bevraging is een losse HTTP-aanroep naar de tussenpartij. Er is geen
sessie, geen paginering en geen doorlopende verbinding.

## 3. Wanneer de BRP wordt bevraagd

De BRP wordt uitsluitend bevraagd voor de kandidaten op een lijst die het CSB
in e-KS heeft geïmporteerd: de kandidaten die de politieke groepering heeft
aangeleverd en de kandidaten die het CSB bij een papieren correctie heeft
toegevoegd. De lijstinleveraar wordt niet bevraagd; bij de Eerste
Kamerverkiezing is die er niet. Een controle is altijd een **controleronde**
over één geïmporteerde lijst: elke kandidaat op die lijst voor wie nog geen
resultaat is vastgelegd, wordt gecontroleerd. De ronde draait op de
achtergrond; de gebruiker wacht er niet op.

Een controleronde start in drie situaties:

1. **Bij de import.** Direct nadat het CSB een ingeleverd pakket aan de hand
   van de hash heeft geïmporteerd, start de ronde automatisch. Dit geldt zowel
   voor de officiële inlevering als voor de voorinlevering.
2. **In de voorinleveringscontrole.** Een lid van het CSB start of hervat de
   ronde met de knop *Controleren met de BRP* op de pagina van de groepering.
3. **In het onderzoek.** Een lid van het CSB start de ronde met dezelfde knop
   op de pagina van de politieke groepering, bijvoorbeeld voor een lijst
   waarvan de ronde voortijdig is gestopt.

Het verloop van een ronde:

- Per lijst draait hooguit één ronde tegelijk. Een tweede verzoek terwijl er
  al een ronde loopt, doet niets.
- De start wordt vastgelegd als status *in uitvoering*, met het tijdstip.
- De ronde controleert de **gecorrigeerde** gegevens van de kandidaten, dus
  inclusief papieren en ambtshalve correcties, omdat dat de gegevens zijn die
  het CSB onderzoekt.
- Kandidaten voor wie al een resultaat is vastgelegd, worden overgeslagen. Een
  hervatte ronde controleert dus alleen wat nog niet gecontroleerd is.
- De kandidaten worden in groepen van tien bevraagd, met één seconde pauze
  tussen de groepen.
- Na elke groep worden eerst alle verstuurde bevragingen in het auditlog
  vastgelegd, en daarna de bevindingen per kandidaat.
- Als alle groepen zijn verwerkt, wordt de status *afgerond* vastgelegd.

Twee bijzondere gevallen:

- **Kandidaat gewijzigd tijdens de bevraging.** Als de gegevens van een
  kandidaat zijn gewijzigd terwijl de BRP werd bevraagd, wordt het resultaat
  niet vastgelegd. De bevindingen gaan immers over gegevens die niet meer op
  het scherm staan. De kandidaat blijft "niet gecontroleerd" en wordt bij een
  volgende ronde opnieuw meegenomen.
- **Ambtshalve correctie na de controle.** Een ambtshalve correctie van een
  kandidaat verwijdert de bevindingen van die kandidaat. De kandidaat toont
  dan de melding dat de gegevens na de controle zijn gewijzigd en opnieuw
  gecontroleerd moeten worden, totdat het CSB een nieuwe ronde start.

## 4. Wat wordt verstuurd

e-KS gebruikt twee bevragingstypen van de API. Beide gaan als bericht naar de
tussenpartij. In beide gevallen is de inhoud een JSON-bericht waarvan het veld
`type` aangeeft om welk type het gaat. De
veldnamen in `fields` zijn de namen die de API voorschrijft. Het bericht wordt
in het auditlog vastgelegd precies zoals het is verstuurd.

### 4.1 Raadplegen op burgerservicenummer

Type `RaadpleegMetBurgerservicenummer`. Bij de Eerste Kamerverkiezing is dit
de hoofdbevraging. Per groep van tien kandidaten wordt één bericht verstuurd
met daarin de burgerservicenummers van alle kandidaten in die groep die er een
hebben.

```json
{
  "type": "RaadpleegMetBurgerservicenummer",
  "burgerservicenummer": ["999992806", "999993653"],
  "fields": [
    "burgerservicenummer",
    "naam.voorletters",
    "naam.voorvoegsel",
    "naam.geslachtsnaam",
    "geslacht",
    "geboorte.datum",
    "verblijfplaats.verblijfadres.woonplaats",
    "overlijden.datum",
    "nationaliteiten.nationaliteit",
    "uitsluitingKiesrecht",
    "partners.naam.voorvoegsel",
    "partners.naam.geslachtsnaam"
  ]
}
```

De lijst `fields` is de volledige set persoonsgegevens die e-KS over een
kandidaat opvraagt. Er wordt niets anders opgevraagd. De set is toegesneden op
de Eerste Kamerverkiezing (en de Tweede Kamerverkiezing). Andere verkiezingen
vragen om meer gegevens die nu nog niet worden opgevraagd: voor de
gemeenteraadsverkiezing de gemeente van inschrijving, voor de verkiezing van
het Europees Parlement de aanduiding bijzonder Nederlanderschap. Het
correspondentieadres ontbreekt bewust: alleen de woonplaats wordt op de
kandidatenlijst gedrukt, dus alleen de woonplaats wordt gecontroleerd. De namen
van partners worden opgevraagd omdat een kandidaat volgens de Kieswet ook onder
de naam van een (ex-)echtgenoot of geregistreerd partner op de lijst mag staan.

Kandidaten zonder burgerservicenummer (niet ingevuld, of expliciet vastgelegd
als "heeft geen") staan niet in dit bericht. Als geen enkele kandidaat in de
groep een burgerservicenummer heeft, wordt dit bericht niet verstuurd.

### 4.2 Zoeken op achternaam en geboortedatum

Type `ZoekMetGeslachtsnaamEnGeboortedatum`. Dit bericht wordt verstuurd voor
een kandidaat van wie het burgerservicenummer geen persoon opleverde, of die er
geen heeft, mits de kandidaat een achternaam én een geboortedatum heeft. Er
wordt alleen het burgerservicenummer opgevraagd: het doel is de persoon te
identificeren, niet om te vergelijken.

```json
{
  "type": "ZoekMetGeslachtsnaamEnGeboortedatum",
  "geslachtsnaam": "Bruin",
  "geboortedatum": "1990-12-11",
  "voorvoegsel": "de",
  "geslacht": "V",
  "inclusiefOverledenPersonen": true,
  "fields": ["burgerservicenummer"]
}
```

- `geslachtsnaam` is de achternaam zoals ingevoerd, zonder voorvoegsel. De BRP
  vergelijkt die exact.
- `geboortedatum` heeft de vorm jaar-maand-dag.
- `voorvoegsel` en `geslacht` (`M` of `V`) staan alleen in het bericht als ze
  worden meegegeven, zie hieronder.
- `inclusiefOverledenPersonen` is altijd waar, zodat een overleden kandidaat
  wordt gevonden en als overleden wordt gemeld, in plaats van als onbekend.

**Verfijnen.** Het eerste zoekbericht bevat alleen achternaam en geboortedatum.
Levert dat meer dan één persoon op, dan volgen hooguit drie verfijnde
berichten, elk met een extra gegeven dat de kandidaat daadwerkelijk heeft, in
deze volgorde:

1. plus het geslacht, als de kandidaat dat heeft opgegeven;
2. plus het voorvoegsel, als de kandidaat dat heeft;
3. plus beide, als de kandidaat beide heeft.

Het verfijnen stopt zodra een bericht precies één persoon of niemand oplevert.
Eén kandidaat leidt dus tot hooguit vier zoekberichten, en meestal tot één.

### 4.3 Vervolgbevraging

Levert het zoeken precies één persoon op, dan wordt het volledige record van
die persoon opgehaald met een bericht van het type
`RaadpleegMetBurgerservicenummer` voor dat ene burgerservicenummer, met
dezelfde `fields` als in 4.1. Daarmee kan de kandidaat alsnog op alle velden
worden vergeleken. Omdat deze vergelijking plaatsvindt met een persoon die op
de overige gegevens is gevonden en niet op het opgegeven burgerservicenummer,
wordt dat als aparte bevinding bij de kandidaat gemeld (zie 6.1).

### 4.4 Aantal bevragingen

Voor een groep van tien kandidaten met een correct burgerservicenummer
verstuurt e-KS één bericht. Voor elke kandidaat van wie het
burgerservicenummer niets oplevert, komen daar hooguit vier zoekberichten en
één vervolgbevraging bij.

## 5. Wat wordt ontvangen

Het antwoord van de tussenpartij is een JSON-bericht met hetzelfde `type` als
de vraag en een lijst `personen`, in de vorm van de Haal Centraal-API. e-KS
leest daaruit alleen de gegevens die het heeft opgevraagd.

```json
{
  "type": "RaadpleegMetBurgerservicenummer",
  "personen": [
    {
      "burgerservicenummer": "999992806",
      "naam": { "geslachtsnaam": "Bruin", "voorvoegsel": "de", "voorletters": "T." },
      "geslacht": { "code": "V" },
      "geboorte": { "datum": { "datum": "1990-12-11" } },
      "overlijden": { "datum": { "datum": "2026-01-31" } },
      "nationaliteiten": [{ "nationaliteit": { "code": "0001" } }],
      "uitsluitingKiesrecht": { "uitgeslotenVanKiesrecht": false },
      "verblijfplaats": { "type": "Adres", "verblijfadres": { "woonplaats": "Utrecht" } },
      "partners": [{ "naam": { "geslachtsnaam": "Jansen" } }]
    }
  ]
}
```

| Gegeven in het antwoord | Vorm | Waarvoor gebruikt |
| --- | --- | --- |
| `burgerservicenummer` | tekst | Het antwoord koppelen aan de juiste kandidaat in de groep |
| `naam.voorletters` | tekst | Vergelijken met de voorletters van de kandidaat |
| `naam.voorvoegsel` | tekst | Vergelijken met het voorvoegsel van de kandidaat |
| `naam.geslachtsnaam` | tekst | Vergelijken met de achternaam van de kandidaat |
| `geslacht.code` | `M`, `V` of `O` (onbekend) | Vergelijken met het geslacht van de kandidaat |
| `geboorte.datum.datum` | jaar-maand-dag; ontbreekt bij een onvolledige datum | Vergelijken met de geboortedatum van de kandidaat |
| `overlijden` | aanwezig als een overlijden is geregistreerd | Verkiesbaarheid: overleden |
| `overlijden.datum.datum` | jaar-maand-dag; ontbreekt bij een onvolledige datum | De datum in de bevinding "overleden" |
| `nationaliteiten[].nationaliteit.code` | code uit de landelijke tabel Nationaliteiten | Verkiesbaarheid: code `0001` (Nederlandse) moet voorkomen |
| `uitsluitingKiesrecht.uitgeslotenVanKiesrecht` | waar/onwaar | Verkiesbaarheid: uitsluiting van het kiesrecht |
| `verblijfplaats.type` | `Adres`, `Locatie`, `VerblijfplaatsBuitenland` of `VerblijfplaatsOnbekend` | Bepalen welke bevinding over de verblijfplaats van toepassing is |
| `verblijfplaats.verblijfadres.woonplaats` | tekst, alleen bij type `Adres` | Vergelijken met de woonplaats van de kandidaat |
| `partners[].naam.voorvoegsel` en `.geslachtsnaam` | tekst | De achternamen waaronder de kandidaat op de lijst mag staan |

Enkele regels bij het lezen:

- Elke waarde wordt eerst als tekst bewaard en pas bij het vergelijken
  geïnterpreteerd. Zo kan e-KS het verschil maken tussen "een andere waarde"
  en "een waarde die e-KS niet kan lezen" (zie 6.2).
- Een `verblijfplaats.type` dat e-KS niet kent, wordt behandeld als onbekende
  verblijfplaats, niet genegeerd.
- Een persoon die als *behandeld als Nederlander* is geregistreerd, heeft in
  de BRP geen nationaliteitscode en telt daarom niet als Nederlander. Artikel
  56 van de Grondwet vereist het Nederlanderschap zelf.
- Personen in het antwoord worden op burgerservicenummer aan een kandidaat
  gekoppeld. Personen waar niet om is gevraagd, worden genegeerd.

## 6. Wat wordt vastgelegd

Alles wat de koppeling doet, wordt als gebeurtenis vastgelegd op de
geïmporteerde lijst en is terug te zien in het auditlog van het CSB, onder de
categorie *BRP-controle*. Bij elke gebeurtenis staat welk lid van het CSB de
controle heeft gestart.

### 6.1 Bevraging: *BRP geraadpleegd*

Technische naam `brp_lookup`. Elke bevraging, beantwoord of mislukt, wordt
vastgelegd vóór de bevindingen die eruit volgen, met:

- voor welke kandidaten de bevraging is gedaan;
- het bericht precies zoals het is verstuurd (hoofdstuk 4);
- de uitkomst: ofwel de burgerservicenummers van de personen die de
  tussenpartij teruggaf, ofwel de foutmelding als de bevraging mislukte.

Het auditlog bevat daarmee elk moment waarop gegevens van een kandidaat naar de
tussenpartij zijn gestuurd, inclusief bevragingen die de tussenpartij heeft
geweigerd of die haar nooit hebben bereikt. Wat de tussenpartij vervolgens
richting de BRP doet, is in e-KS niet zichtbaar en dus ook niet vastgelegd.

### 6.2 Bevindingen: *Kandidaat gecontroleerd met de BRP*

Technische naam `brp_person_checked`. Het resultaat voor één kandidaat is een
lijst bevindingen. Een lege lijst betekent: gecontroleerd, en de BRP komt op
alle gecontroleerde velden overeen.

Bevindingen worden naast de gegevens van de kandidaat getoond. Ze worden
bewust niet automatisch een verzuim. Het CSB stelt eerst vast of een verschil
een werkelijk probleem is en kan het ambtshalve corrigeren; alleen wat
overblijft wordt een verzuim. Het CSB kan elke bevinding als *afgehandeld*
markeren (technische naam `set_brp_finding_handled`), en ook dat wordt
vastgelegd.

Hieronder staan alle bevindingen, met de melding zoals e-KS die toont en de
situatie waarin die ontstaat. De technische naam is de naam in de code en in
de vastgelegde gegevens.

**Bevindingen over het opzoeken van de kandidaat**

| Technische naam | Melding | Situatie |
| --- | --- | --- |
| `BsnMissing` | Er is geen burgerservicenummer vastgelegd, waardoor deze kandidaat niet in de BRP is op te zoeken. | De kandidaat heeft geen burgerservicenummer. |
| `BsnNoneConfirmed` | Van deze kandidaat is vastgelegd dat die geen burgerservicenummer heeft, waardoor deze niet in de BRP is op te zoeken. | De groepering heeft expliciet vastgelegd dat de kandidaat er geen heeft. |
| `BsnUnknown` | Er is geen persoon in de BRP met dit burgerservicenummer. | De kandidaat heeft een burgerservicenummer en de BRP gaf er niemand voor terug. |
| `BsnNotUnique` | Er is meer dan één persoon in de BRP met dit burgerservicenummer. | De BRP gaf meer dan één persoon terug. Er wordt niets vergeleken. |
| `BsnMatchedByPersonalDetails` | Op de overige persoonsgegevens is precies één persoon in de BRP gevonden, met burgerservicenummer {nummer}. De gegevens hieronder zijn met die persoon vergeleken. | Eén van de eerste drie bevindingen was van toepassing, en het zoeken op achternaam en geboortedatum leverde precies één persoon op. Alle veldbevindingen die volgen, zijn een vergelijking met die persoon. |
| `PersonalDetailsNotUnique` | Er passen meerdere personen in de BRP op de persoonsgegevens van deze kandidaat, waardoor de gegevens niet zijn vergeleken. | Eén van de eerste drie bevindingen was van toepassing, en het zoeken leverde ook na alle verfijningen meer dan één persoon op. Er wordt niets vergeleken. |

Na `BsnMissing`, `BsnNoneConfirmed` of `BsnUnknown` wordt alleen gezocht als
de kandidaat een achternaam én een geboortedatum heeft. Anders blijft het bij
die ene bevinding en wordt er geen zoekbericht verstuurd.

**Bevindingen over één veld**

Elk gecontroleerd veld (voorletters, voorvoegsel, achternaam, geslacht,
geboortedatum, woonplaats) wordt afzonderlijk vergeleken en levert hooguit één
van deze bevindingen op:

| Technische naam | Melding | Situatie |
| --- | --- | --- |
| `Mismatch` | {veld} wijkt af van de BRP, waarin staat: {waarde} | De BRP heeft een waarde die e-KS kan lezen, en die verschilt van die van de kandidaat. De BRP-waarde wordt bij een ambtshalve correctie als suggestie aangeboden. |
| `Unparsable` | De waarde uit de BRP voor {veld} is niet te interpreteren en is daarom niet vergeleken: {waarde} | De BRP heeft een waarde die e-KS niet als geldige waarde voor dat veld accepteert, bijvoorbeeld voorletters met een cijfer of een datum in een andere notatie. Er is niets vergeleken. De ruwe waarde wordt getoond, maar nooit als correctie aangeboden. |
| `MissingInBrp` | De BRP bevat geen {veld} voor deze kandidaat. | Het veld is opgevraagd, maar de BRP gaf het niet of leeg terug. |
| `LastNameNotAllowed` | De achternaam mag volgens de BRP zijn: {namen} | Voorvoegsel en achternaam van de kandidaat komen samen met geen van de namen overeen waaronder de kandidaat volgens de Kieswet op de lijst mag staan. De melding noemt elke naam die wél zou mogen. |

Regels per veld:

- **Voorletters en voorvoegsel.** Een ontbrekende waarde in de BRP is hier een
  waarde, geen gat. Een kandidaat zonder voorletters of voorvoegsel komt
  overeen met een BRP-record zonder. Alleen een kandidaat die er wél een heeft,
  krijgt de melding dat de BRP er geen heeft.
- **Geslacht.** Wordt alleen vergeleken als de kandidaat een geslacht heeft
  opgegeven. De BRP-code `O` (onbekend) leidt tot *de BRP bevat geen geslacht*,
  niet tot *niet te interpreteren*.
- **Geboortedatum.** Een onvolledige datum in de BRP (alleen een jaar) leidt
  tot *de BRP bevat geen geboortedatum*.
- **Achternaam en voorvoegsel.** Worden samen vergeleken met alle namen
  waaronder de kandidaat mag staan: de eigen naam, de naam van elke
  (ex-)echtgenoot of geregistreerd partner, en beide namen met een koppelteken
  in beide volgordes. Past de naam bij geen daarvan en heeft de BRP partners
  geregistreerd, dan volgt één bevinding `LastNameNotAllowed`. Zonder
  partners, of als de eigen naam in de BRP niet leesbaar is, worden
  voorvoegsel en achternaam elk apart gemeld volgens de tabel hierboven.
- **Woonplaats.** Alleen de woonplaats wordt vergeleken, en alleen als de
  verblijfplaats van het type `Adres` is. Andere typen leiden tot een van de
  bevindingen hieronder.

**Bevindingen over de verblijfplaats**

| Technische naam | Melding | Situatie |
| --- | --- | --- |
| `ResidenceAbroad` | Deze kandidaat woont in het buitenland, waardoor de BRP geen woonplaats bevat. | `verblijfplaats.type` is `VerblijfplaatsBuitenland`. |
| `ResidenceWithoutAddress` | Deze kandidaat heeft geen woonadres in de BRP (bijvoorbeeld een briefadres), waardoor de BRP geen woonplaats bevat. | `verblijfplaats.type` is `Locatie`. |
| `ResidenceUnknown` | De verblijfplaats van deze kandidaat is onbekend in de BRP. | `verblijfplaats.type` is `VerblijfplaatsOnbekend`, een type dat e-KS niet kent, of de verblijfplaats ontbreekt. |

**Bevindingen over de verkiesbaarheid** (artikel 56 Grondwet; de leeftijdseis
wordt gecontroleerd op de eigen gegevens van de kandidaat, niet via de BRP)

| Technische naam | Melding | Situatie |
| --- | --- | --- |
| `Deceased` | Volgens de BRP is deze kandidaat overleden op {datum}. Of, zonder volledige datum: Volgens de BRP is deze kandidaat overleden, zonder volledige datum van overlijden. | De BRP heeft een overlijden geregistreerd. |
| `NotDutch` | Volgens de BRP heeft deze kandidaat niet de Nederlandse nationaliteit. | Geen enkele nationaliteit in het antwoord heeft code `0001`. |
| `ExcludedFromSuffrage` | Volgens de BRP is deze kandidaat uitgesloten van het kiesrecht. | `uitsluitingKiesrecht.uitgeslotenVanKiesrecht` is waar. |

### 6.3 Status van de controleronde: *Status BRP-controle gezet*

Technische naam `set_brp_status`. De lijst heeft altijd één status:

| Status | Betekenis | Melding bij de kandidaten |
| --- | --- | --- |
| Niet gestart (`not_started`) | Er is nooit een ronde gestart. | De kandidaten zijn nog niet tegen de BRP gecontroleerd. |
| In uitvoering (`in_progress`) | Een ronde is gestart, met het tijdstip. | Zolang de ronde loopt: De controle tegen de BRP loopt nog, waardoor nog niet alle kandidaten zijn gecontroleerd. Als de applicatie is herstart terwijl de ronde liep, blijft deze status achter zonder lopende ronde en toont e-KS: De controle tegen de BRP is voortijdig gestopt, waardoor niet alle kandidaten zijn gecontroleerd. Start de controle opnieuw om de rest te controleren. |
| Voortijdig gestopt (`aborted`) | De ronde is door een fout gestopt. Kandidaten die vóór dat moment zijn gecontroleerd, behouden hun bevindingen; de rest is niet gecontroleerd. | De controle tegen de BRP is voortijdig gestopt, waardoor niet alle kandidaten zijn gecontroleerd: {foutmelding} |
| Afgerond (`finished`) | Elke kandidaat is gecontroleerd. | Geen, tenzij een kandidaat daarna is gecorrigeerd: De gegevens zijn na de controle tegen de BRP gewijzigd en moeten opnieuw gecontroleerd worden. |

## 7. Fouten

De koppeling houdt twee zaken strikt gescheiden. Alles wat de BRP *over een
kandidaat* zegt, is een bevinding (6.2) en nooit een fout. Een fout betekent
dat de tussenpartij *niet bevraagd kon worden* of *geen bruikbaar antwoord
gaf*, en zegt nooit iets over een persoon. e-KS kan daarbij niet zien of een
probleem bij de tussenpartij zelf ligt of bij haar verbinding met de BRP; in
beide gevallen ziet e-KS alleen het antwoord van de tussenpartij.

### 7.1 Fouten bij één bevraging

| Situatie | Fout | Moment |
| --- | --- | --- |
| De tussenpartij is niet bereikbaar: verbinding geweigerd, adres niet gevonden, certificaatprobleem, time-out | Upstream-fout (`UpstreamError`) met de melding van de HTTP-bibliotheek | Bij het versturen |
| De tussenpartij antwoordt met een HTTP-foutstatus (4xx of 5xx), bijvoorbeeld een geweigerde API-sleutel, een storing bij de tussenpartij of een storing in haar verbinding met de BRP | Upstream-fout (`UpstreamError`) met de HTTP-status | Bij het ontvangen van het antwoord |
| Het antwoord is geen geldige JSON of niet een van de twee verwachte antwoordtypen | Upstream-fout (`UpstreamError`) | Bij het lezen van het antwoord |
| Er worden meer dan tien kandidaten in één groep aangeboden | BRP-fout (`BrpError`), een interne controle | Vóór het versturen; er wordt niets verstuurd |

Elke mislukte bevraging wordt alsnog als *BRP geraadpleegd* in het auditlog
vastgelegd, met als uitkomst de foutmelding. Alleen een bevraging die nooit is
verstuurd (de laatste regel) laat geen spoor na in het auditlog.

### 7.2 Gevolg voor de controleronde

Een fout bij een bevraging stopt de hele ronde. De status van de lijst wordt
*voortijdig gestopt* met de foutmelding, en de fout wordt in het
applicatielog geschreven. Een onverwachte crash van de achtergrondtaak wordt op
dezelfde manier afgehandeld. De foutmelding is de volledige tekst van de fout,
bijvoorbeeld `Upstream error: error sending request for url (...)`.

Een onbereikbare tussenpartij lijkt daardoor nooit op een groep kandidaten
zonder bevindingen. De kandidaten in de mislukte groep en in alle latere groepen
blijven "niet gecontroleerd". Het CSB ziet de status met de foutmelding bij de
lijst en kan een nieuwe ronde starten, die verdergaat waar de vorige stopte.

Ook een fout bij het *vastleggen* van een resultaat (een opslagprobleem) stopt
de ronde op deze manier. Een gelijktijdige wijziging van de lijst tijdens het
vastleggen is geen fout: het vastleggen wordt tot drie keer opnieuw geprobeerd,
en lukt het dan nog niet, dan wordt het resultaat weggegooid met een
waarschuwing in het applicatielog en blijft de kandidaat "niet gecontroleerd".

### 7.3 Wat de gebruiker direct te zien krijgt

Het verzoek dat een ronde start, wacht alleen tot de status *in uitvoering* is
vastgelegd. Het kan dus niet mislukken door de tussenpartij of de BRP. De gebruiker ziet een
mislukte ronde aan de status bij de lijst (6.3), niet aan het verzoek waarmee
de ronde is gestart.

Mocht een BRP-fout of upstream-fout toch tot een foutpagina leiden, dan is dat
een algemene interne serverfout (HTTP 500) zonder nadere toelichting. De
inhoud van de fout komt dan alleen in het applicatielog en, voor de bevraging
zelf, in het auditlog.

### 7.4 Configuratiefouten

Deze fouten treden op bij het starten van de applicatie, niet tijdens een
controle:

| Situatie | Fout |
| --- | --- |
| `BRP_BASE_URL` of `BRP_API_KEY` is niet ingesteld | Ontbrekende omgevingsvariabele (`ConfigLoadError`, ``missing field `brp_base_url` ``) |
| `BRP_TIMEOUT` is geen getal | Configuratiefout: *Invalid BRP_TIMEOUT; please enter a number* |

## 8. Testen

De geautomatiseerde tests bereiken nooit de tussenpartij of de BRP. Ze draaien
tegen een ingebouwde nabootsing (`src/test_support/brp_stub.rs`) die beide
bevragingstypen beantwoordt zoals de API van de tussenpartij dat doet: exacte
vergelijking op achternaam en geboortedatum, verfijning op de optionele
gegevens, en overleden personen alleen als daarom is gevraagd. De nabootsing
legt elke ontvangen bevraging vast, zodat de tests ook controleren wát er is
verstuurd. De container `personen-mock` wordt alleen gebruikt om de applicatie
lokaal te draaien en voor één handmatige rooktest.
