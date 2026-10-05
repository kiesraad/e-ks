# Werkwijze e-KS team: kwaliteit waarborgen

Dit document beschrijft de werkwijze van het e-KS team en hoe er kwaliteit wordt gewaarborgd. Kwaliteit is hierbij geen aparte stap aan het einde van het ontwikkelproces, maar een geïntegreerd onderdeel van de werkwijze.

Er is aandacht voor de technische kwaliteit en functionele kwaliteit. Functionele kwaliteit wordt gewaarborgd door de applicatie te toetsen aan wetgeving en gebruikersbehoeftes.

De belangrijkste kwaliteitsattributen voor de ontwikkeling van e-KS zijn:
- Betrouwbaarheid: Kan je de software vertrouwen?
- Bruikbaarheid: Is de software makkelijk te gebruiken (voor alle bedoelde gebruikers)?
- Beveiliging: Biedt de software voldoende bescherming tegen ongewenst gebruik?
- Testbaarheid: Is het eenvoudig om de software te testen en controleren?
- Onderhoudbaarheid: Kan de software eenvoudig worden onderhouden en aangepast?
- Beheerbaarheid: Is de software eenvoudig te installeren, ondersteunen en onderhouden?

## Ontwikkeling

### Algemene werkwijze
- Er wordt gezorgd voor continue kwaliteit door **iteratief en incrementeel te ontwikkelen**:
  - Het e-KS team werkt [agile](https://agilemanifesto.org/) door middel van [Scrum](https://www.scrum.org/)
  - Het e-KS team werkt in tweewekelijkse sprints, inclusief alle Scrum ceremonies
  - Refinement wordt deels gedaan tijdens de sprint planning en deels ad hoc voordat er wordt begonnen met de ontwikkeling van nieuwe functionaliteit
  - De Product Owner stemt zowel voor als na de ontwikkeling de functionaliteit af met interne stakeholders
  - Testen worden gelijktijdig met de ontwikkeling gemaakt en uitgevoerd
  - Nieuwe features worden ingediend door middel van Pull Requests
  - De stappen van het oppakken van een issue tot het mergen van de Pull Request staan beschreven in de [Definition of Doing](definition-of-doing.md)
  - Pull Requests worden pas geaccepteerd in de main branch zodra:
     - De CI/CD pipeline, met quality gates, slaagt
     - De wijziging door minstens één andere ontwikkelaar is gereviewd en goedgekeurd (vierogenprincipe)
     - Alle opmerkingen op de Pull Request zijn afgehandeld
  - Deze regels zijn technisch afgedwongen met branch protection op `main`:
     - Minstens één goedkeurende review, waarbij goedkeuringen vervallen zodra er nieuwe commits worden gepusht
     - Alle verplichte checks uit de CI/CD pipeline zijn geslaagd
     - Alle conversaties op de Pull Request zijn afgehandeld
     - Commits zijn ondertekend (signed commits)
     - Force pushen naar en verwijderen van `main` is niet mogelijk
     - De regels gelden ook voor beheerders, er zijn geen uitzonderingen
- Er wordt gezorgd voor een gezamenlijk en objectief kwaliteitsniveau door de **Definition of Ready (DoR) en Definition of Done (DoD)**:
  - DoR: issue mag alleen worden opgepakt als er is voldaan aan de [DoR](definition-of-ready.md)
  - DoD: issue is pas klaar als er is voldaan aan de [DoD](definition-of-done.md)
- Er wordt gezorgd voor blijvend begrip van de code door de **[AI Usage Policy](ai-policy.md)**: AI mag de ontwikkeling versnellen, maar nooit het begrip van de ontwikkelaar vervangen

### Continuous Integration / Continuous Delivery (CI/CD) pipeline

CI/CD helpt om risico's te mitigeren. Door codewijzigingen regelmatig (bij elke PR) te integreren, testen en opleveren, worden fouten eerder ontdekt en wordt de kans op problemen bij het mergen en opleveren van nieuwe code verkleind. Door een CI/CD-pipeline in te richten kunnen deze stappen geautomatiseerd worden gedaan.

Sigrid (van SIG) en SonarQube worden ingezet voor geautomatiseerde analyse en borging van de (technische) kwaliteit van de code. Ze analyseren de code onder andere op onderhoudbaarheid, architectuur, beveiliging en voorspelbaarheid. Sigrid en SonarQube zijn geïntegreerd in de CI/CD pipeline.

De CI/CD pipeline binnen e-KS bevat de volgende verplichte quality gates. Deze blokkeren het mergen van een Pull Request:
- Testen
  - Alle Playwright testen moeten slagen
  - Alle Rust testen moeten slagen
- Code style
  - De Rust code is geformatteerd volgens `cargo fmt`
  - Linters geven geen errors of warnings (clippy voor Rust, Biome voor TypeScript en CSS)
  - Geen djlint format warnings op HTML templates
  - De rustdoc documentatie bouwt zonder warnings
  - Alle tekstbestanden eindigen op een newline
  - De CI/CD pipeline configuratie mag geen zizmor linting errors bevatten
- Dependency management
  - Er mogen geen Rust crates worden toegevoegd die niet aan onze kwaliteit/licensing standaarden voldoen (zie deny.toml voor exacte configuratie)
- Overig
  - De release build slaagt en het resultaat wordt naar de testomgeving gepubliceerd
  - De titel van de Pull Request verwijst naar minstens één issue
  - De gegenereerde PDF modellen worden vergeleken met die op `main`, verschillen worden als comment op de Pull Request geplaatst

Daarnaast draaien de volgende checks adviserend: ze rapporteren wel een uitkomst, maar blokkeren het mergen niet:
- Algemene kwaliteit
  - Sigrid score van minimaal 3.5 ster op nieuwe code, de uitkomst wordt als comment op de Pull Request geplaatst
  - SonarQube quality gate op nieuwe code (we gebruiken hiervoor de default "Sonar way" configuratie, met onder andere een test coverage van minimaal 80% op nieuwe code, gemeten over de Rust code)
- Architectuur
  - Cyclische dependencies tussen componenten (de top level directories onder `src/`) zijn niet toegestaan, dit wordt gecontroleerd door dylint met de [dylint-module-cycles](https://github.com/tweedegolf/dylint-module-cycles) lint
- Proces
  - Alle checkboxes van de DoD checklist in de beschrijving van de Pull Request zijn afgevinkt

## Juiste functionaliteit

Om tot de juiste functionaliteit te komen werkt het e-KS team met use cases (gebruiksscenario's), deze zijn afgeleid van de Kieswet. Vanuit deze use cases worden de epics gemaakt. Vanuit de epics worden issues aangemaakt die door de ontwikkelaars worden opgepakt. Vervolgens worden de issues getest door de Product Owner. Daarnaast wordt de applicatie getest door interne en externe stakeholders.

### Interne stakeholders
Frequentie: wekelijks  
Wie: afdelingen "Juridische Kennis en Advies" en "Regie Kwaliteit Verkiezingsketen"  
Doel: Vooraf en achteraf feedback ophalen  
Hoe:  
- Refinement van de komende functionaliteit
- Review (demo) van gebouwde functionaliteit
- Testen van functionaliteit op functioneel niveau
- Testen van functionaliteit op juridische kaders

### Externe stakeholders
Frequentie: meerdere keren per jaar  
Wie: Zowel politieke partijen als gemeentes en waterschappen  
Doel: Vooraf en achteraf feedback ophalen  
Hoe:  
- Review (demo) van gebouwde functionaliteit
- Testen van functionaliteit op functioneel niveau

## Testen

De volgende testen worden uitgevoerd om de kwaliteit van de software te borgen:

- Exploratief testen: Testen per issue of er bugs of verbeterpunten zijn.
- Rust unit testen: Testen onderdelen van de back-end. Nieuwe code wordt afgedekt met unit testen, met een coverage van minimaal 80%.
- Playwright testen: Testen applicatie als geheel. Twee soorten testen:
  - 1) Testen gekoppeld aan use cases, use cases gekoppeld aan wetgeving.
  - 2) End-to-end testen vanuit het gebruikersperspectief, op epic-niveau.
- Playwright-unit testen: Testen de front-end scripts.
- Testen door de Product Owner (PO): elk issue wordt getest door de PO om te kijken of de gewenste functionaliteit is ontwikkeld.
- Testen met eindgebruikers: Meerdere keren per jaar wordt er getest met eindgebruikers. Er wordt dan gekeken of de software voldoet aan hun behoeftes.

## Externe kwaliteitstoetsen

- DigiD audit
- Audit en/of penetratietest: voorafgaand aan elke major release, zie de [DoD](definition-of-done.md)
