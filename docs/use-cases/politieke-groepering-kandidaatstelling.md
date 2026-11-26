# Use cases politieke groepering - kandidaatstelling

## Politieke groepering maakt de set benodigde documenten (vlieger) art. R

__Niveau:__ Hoog-over, vlieger, 🪁

### Hoofdscenario en uitbreidingen

__Hoofdscenario:__

1. [Politieke groepering zet alle algemene lijstgegevens in de applicatie.](#politieke-groepering-zet-alle-algemene-lijstgegevens-in-de-applicatie-zee-art-r-voor-eerste-kamerverkiezing)
2. [Politieke groepering maakt de kandidatenlijsten.](#politieke-groepering-maakt-de-kandidatenlijsten-zee-art-r-voor-eerste-kamerverkiezing)
3. Politieke groepering machtigt de lijstinleveraar.
4. [Kandidaten geven instemming met hun plek op de lijst.](#kandidaten-geven-instemming-met-hun-plek-op-de-lijst-zee)
5. [De lijstinleveraar stelt vast alle gegevens volledig zijn.](#de-lijstinleveraar-stelt-vast-alle-gegevens-volledig-zijn-en-downloadt-en-print-alle-benodigde-documenten-zee)
6. [De lijstinleveraar downloadt en print alle benodigde documenten.](#de-lijstinleveraar-stelt-vast-alle-gegevens-volledig-zijn-en-downloadt-en-print-alle-benodigde-documenten-zee)

__Uitbreidingen__:

0. Partij heeft geen zetels in vertegenwoordigend orgaan, dus waarborgsom en ondersteuningsverklaringen.


## Gebruiker politieke groepering logt voor de eerste keer in (zee)

__Niveau:__ Gebruikersdoel, zee, 🌊

__Hoofdscenario:__

1. De gebruiker logt in met DigiD.


## Politieke groepering zet alle algemene lijstgegevens in de applicatie (zee), art. R (voor Eerste Kamerverkiezing)

__Niveau:__ Gebruikersdoel, zee, 🌊


### Hoofdscenario en uitbreidingen

__Hoofdscenario:__

1. De gebruiker selecteert dat ze meedoen met een opzichzelfstaande geregistreerde naam.
2. De gebruiker vult in dat er bij de vorige verkiezing 16 of meer zetels zijn behaald en mag hierdoor maximaal 80 kandidaten op de lijst plaatsen **art. R 4 (voor Eerste Kamerverkiezing)** 
3. De gebruiker vult de aanduiding van de politieke groepering in. **art. R (voor Eerste Kamerverkiezing)** 
4. De gebruiker vult de volledige statutaire naam en de gegevens van de gemachtigde in.
5. Politieke groepering vult de lijstinleveraar en vervangers herstel van verzuimen in **art. R 7 (voor Eerste Kamerverkiezing)**
6. De gebruiker vult de gegevens van de lijstinleveraar in en de vervangers voor herstel verzuim, dit kunnen oo bestaande kandidaten zijn. *de stap is hier logisch als je kijkt naar welke gebruiker het doet, maar moet per lijst te kiezen zijn* **art. R 7 (voor Eerste Kamerverkiezing)**


__Uitbreidingen__:   
1a. [De politieke groepering kiest ervoor om de lijst samen in te leveren](#de-politieke-groepering-kiest-ervoor-om-de-lijst-samen-in-te-leveren)  
1b. De politieke groepering doet mee met een blanco lijst.  
  1b1. De politieke groepering hoeft geen gemachtigde te selecteren  
2a. De politieke groepering vult in dat ze minder dan 16 zetels hebben behaald en mogen daardoor maximaal 50 kandidaten op de lijst plaatsen  

### Open punten

- Check wie welke stap doet. >> Fase 1, een gebruiker kan iedereen zijn. Authenticatie wordt nog schriftelijk gedaan
- Checken of inderdaad voor elke H1 een andere lijstinleveraar >> Ja dat mag

## Politieke groepering maakt de kandidatenlijsten (zee) art. R (voor Eerste Kamerverkiezing)

__Niveau:__ Gebruikersdoel, zee, 🌊

__Hoofdscenario:__

1. Politieke groepering maakt een lijst aan
2. Politieke groepering vinkt aan voor welke gebieden de lijst geldig is **art. R 10 (voor Eerste Kamerverkiezing)**
3. Politieke groepering vult de lijst door personen toe te voegen
4. De lijstinleveraar vult de benodigde personalia ([Personalia kandidaat H1](./data.md#personalia-kandidaat-h1-art-h2-kiesbesluit)) in.
5. De applicatie valideert de ingevoerde gegevens en geeft feedback. **art. R 5 (voor Eerste Kamerverkiezing)**
6. Politieke groepering download alle modellen vanuit het systeem.

Optioneel vervolg: door naar stap 1

__Uitbreidingen__:

- 1a. Politieke groepering selecteert een bestaande lijst als sjabloon
- 3a. Politieke groepering vult de lijst en past de volgorde van de kandidaten aan

## Politieke groepering machtigt de lijstinleveraar (zee) - Nice to have (Fase 2)

__Niveau:__ Gebruikersdoel, zee, 🌊

**afhankelijk van e-Herkenning**

## Kandidaten geven instemming met hun plek op de lijst (zee) - Nice to have (Fase 2)

__Niveau:__ Gebruikersdoel, vlieger, 🪁 

1. Lijstinleveraar geeft BSN's van kandidaten door op de kandidatenlijst
2. Lijstinleveraar bericht kandidaten dat ze kunnen inloggen op de applicatie
3. Kandidaten loggen in op de applicatie met DigiD
4. De applicatie maakt een koppeling met de BRP
5. Kandidaat bevestigt dat de gegevens uit de BRP correct zijn
6. Kandidaat bekijkt de kandidatenlijst
7. Kandidaat stemt in met de plek op de kandidatenlijst
8. De applicatie maakt een model H9 aan voor elke kandidaat die heeft ingestemd. Op het model H9 komt een vinkje te staan dat de kandidaat heeft ingestemd met DigiD. 

__Uitbreidingen__:

- 3a. Kandidaat logt niet in met DigiD
  - 3a1. Kandidaten zetten hun handtekening op de H9
- 5a. Kandidaat ziet dat de gegevens niet goed zijn
  - 5a1. De kandidaat ast de gegevens aan
  - 5a2. De applicatie geeft een waarschuwing dat de gegevens niet overeenkomen met de BRP
  - 5a3. De kandidaat bevestigt gegevens
- 7a. Kandidaat stemt niet in met de plek op de lijst.
  - 7a1. Er wordt geen H9 voor de kandidaat aangemaakt


__Open punten__:
1. Is BSN verplicht? @Richard: ja doordat het op model H9 staat.

## De lijstinleveraar stelt vast alle gegevens volledig zijn en downloadt en print alle benodigde documenten. (zee)

__Niveau:__ Gebruikersdoel, vlieger, 🪁

__Hoofdscenario:__

1. De lijstinleveraar stelt vast dat alle kandidaten correct op de kandidatenlijst staan.
2. De lijstinleveraar stelt vast dat de gemachtigde, lijstinleveraar, verkiezing, aanduiding en kieskringen correct zijn ingevuld.
3. De lijstinleveraar bevestigt dat alles compleet is.
4. De applicatie valideert dat alle benodigde gegevens correct zijn ingevuld.
5. De applicatie genereert de benodigde documenten. (LINK naar pagina met alle benodigde documenten per verkiezing)
6. De lijstinleveraar downloadt de benodigde documenten in pdf.
7. De lijstinleveraar print de benodigde documenten uit.


__Uitbreidingen__:

- 1a. er ontbreken kandidaten  
  - 1a1. De lijstinleveraar vult de missende kandidaten in
- 1b. er staan kandidaten op die niet op de lijst horen  
  - 1b1. De lijstinleveraar verwijdert kandidaten
- 1c. gegevens kloppen niet
  - 1c1. De lijstinleveraar corrigeert de gegevens
 
- 2a. gegevens kloppen niet
  - 2a1. De lijstinleveraar corrigeert de gegevens
- 2b. gegevens zijn niet volledig
  - 2b1. De lijstinleveraar vult de gegevens aan
 
- 4a. gegevens kloppen niet
  - 4a1. de applicatie geeft een (blocking) waarschuwing
  - 4a2. De lijstinleveraar corrigeert de gegevens
    - 4a2a. De lijstinleveraar corrigeert de gegevens niet 
- 4b. gegevens zijn niet volledig
  - 4b1. de applicatie geeft een (blocking) waarschuwing
  - 4b2. De lijstinleveraar vult de gegevens aan
    - 4b2b. De lijstinleveraar vult de gegevens niet aan
 
- 8a. er zijn na het printen toch fouten
  - 8a1. De lijstinleveraar geeft in de applicatie aan dat er fouten zijn.
  - 8a2. De lijstinleveraar bewerkt de gegevens
  - hierna kan je door naar stap 3

__Opmerkingen__:
- stappen 1 t/m 3 kan de partij herhalen als ze meerdere lijsten inleveren


## Politieke groepering ontvangt bericht met te maken wijzigingen van Centraal Stembureau (Zee)

__Niveau:__ Gebruikersdoel, zee, 🌊

__Hoofdscenario:__

1. Politieke groepering ontvangt schriftelijk de te maken wijzigingen.
2. Politieke groepering corrigeert de fouten
3. Vervolg zie: [De lijstinleveraar stelt vast alle gegevens volledig zijn en downloadt en print alle benodigde documenten. (zee)]((#de-lijstinleveraar-stelt-vast-alle-gegevens-volledig-zijn-en-downloadt-en-print-alle-benodigde-documenten-zee))

## De politieke groepering kiest ervoor om de lijst samen in te leveren (art. R 7)

__Niveau:__ Gebruikersdoel, zee, 🌊

__Hoofdscenario:__

1. De politieke groepering geeft aan dat ze meedoen met een samengevoegde aanduiding
2. De politieke groepering vult één samengevoegde aanduiding in
3. De politieke groepering vult voor elke partij een gemachtigde in
4. De politieke groepering vult alle statutaire namen in

