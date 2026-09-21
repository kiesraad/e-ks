## Publicatie beveiligingsrapport HackDefense juli 2026

De Kiesraad heeft de beveiliging van de nieuwe kandidaatstellingsoftware laten analyseren door een externe partij, HackDefence. HackDefense is geselecteerd conform de geldende inkoopprocedure. 
Binnen de betreffende mantelovereenkomst voeren drie partijen deze onderzoeken bij de Kiesraad roulerend uit. Voor dit advies is dit HackDefense geweest. 
In het onderstaande rapport is HackDefence positief over de beveiliging van de nieuwe kandidaatstellingssoftware en kennis over beveiliging van het ontwikkelteam. 
Het algemeen oordeel luidt dan ook “Ons algemeen oordeel is dat de betrokkenen bij de ontwikkeling van e-KS zich terdege bewust zijn van de noodzaak dat e-KS veilig kan worden gebruikt."

### Rapport

Hieronder staat het rapport en een tabel met de bevindingen.

[Adviesrapport beveiliging e-KS 24 juli.pdf](https://github.com/user-attachments/files/32461168/Adviesrapport.beveiliging.e-KS.24.juli.pdf)


In onderstaande tabel is aangegeven per advies of wij het advies overnemen of niet. In één geval nemen we het advies deels over. En in één geval nemen we het advies niet over.

|Nr|	Aanbeveling|	Overnemen j/n|
|--|------------|----------------|
|1	|De versleuteling van gegevens in de database en van de applicatieservers vereist sleutelbeheer waarbij goed gedocumenteerd (…)	| Ja |
|2 |	Maak beleid voor de rotatie van sleutelmateriaal per verkiezing.	|Ja |
|3|	Definieer een kritieke periode voorafgaand aan de Dag der Kandidaatstelling (…) |	Ja |
|4|	Maak met de hosting- en/of ontwikkelpartij een service level agreement (SLA) |	Ja |
|5|	Beperk in de firewall inkomend verkeer tot de internet-adressen van de anti-DDoS-provider (…)	|Ja |
|6|	Documenteer vóór de kritieke periode op grond van welke redenen een update tijdens die periode wel of niet wordt doorgevoerd. |	Overnemen arbitrair is lastig; beter documenteren wie mag besluiten en welke overwegingen dan hanteren. Niet arbitrair in de tijd of CVSS als basis. |
|7|	Weeg de toegevoegde waarde van een externe dienstverlener ter bescherming tegen DDoSaanvallen af (…)	| NaWas; het toepassen van NaWas is praktisch beperkt ivm hostingkeuze. Niet elke hostingpartij ondersteund NaWas. Wij vinden de tegenstelling beschikbaarheid versus vertrouwelijkheid niet helemaal zuiver. Geen DDoS bescherming is geen optie wat ons betreft. Dat zou betekenen dat eKS mogelijk onbruikbaar zou zijn. |
|8|	Besteed bij de penetratietest specifiek aandacht aan de SAML-koppeling met DigiD (…)	|Ja|
|9|	Documenteer het autorisatiemodel expliciet (…)	|Ja|
|10|	Aandachtspunt voor de penetratietest: het testen van horizontale privilege-escalatie (…)	|Ja|
|11|	Besteed bij de penetratietest met name aandacht aan zaken die de beschikbaarheid kunnen beïnvloeden, (…)	|Ja|
|12|	Wij adviseren de motivatie voor dit ontwerp en de aanname dat de mastersleutels nooit uitlekken expliciet te documenteren. Envelope encryption is een bekend alternatief, maar neemt het inherente risico rond het BSN niet weg (…)	|Ja|
