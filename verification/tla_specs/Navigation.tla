---------------------------- MODULE Navigation ----------------------------
EXTENDS Naturals, FiniteSets, TLC, Sequences

CONSTANTS
    NavIDs,
    NkIDs,
    NONE

Symmetry == Permutations(NavIDs) \cup Permutations(NkIDs)

BeforeUnloadState  == {"Queued", "Approved", "Aborted"}
NavigationStatus   == {"started", "finalized", "aborted"}
NavigableNavStatus == {"WaitingForUnload", "Finalized", "InitiallyFinalized"}

VARIABLES
    navigables,
    navigations,
    freeNavIDs,
    freeNkIDs,
    navigationStartQueue

vars == <<navigables, navigations, freeNavIDs, freeNkIDs, navigationStartQueue>>

ActiveNavigables  == DOMAIN navigables
ActiveNavigations == DOMAIN navigations

Children(nID) ==
    {n \in ActiveNavigables : navigables[n].parent = nID}

RECURSIVE Descendants(_)
Descendants(nID) ==
    LET kids == Children(nID)
    IN  kids \cup UNION {Descendants(k) : k \in kids}

TypeOK ==
    /\ \A n \in ActiveNavigables :
        /\ navigables[n].parent \in (ActiveNavigables \cup {NONE})
        /\ navigables[n].doc_status \in {"loaded", "unloaded"}
        /\ navigables[n].before_unload_status \subseteq (BeforeUnloadState \X NkIDs)
        /\ navigables[n].nav_status \in NavigableNavStatus
        /\ navigables[n].ongoing_nav_id \in (NkIDs \cup {NONE})
    /\ \A nk \in ActiveNavigations :
        /\ navigations[nk].navigable \in ActiveNavigables
        /\ navigations[nk].status \in NavigationStatus
        /\ navigations[nk].affected \subseteq ActiveNavigables
    /\ freeNavIDs \subseteq NavIDs
    /\ freeNkIDs  \subseteq NkIDs
    /\ \A i \in 1..Len(navigationStartQueue) :
           navigationStartQueue[i] \in ActiveNavigations

Init ==
    /\ navigables  = [x \in {} |-> 0]
    /\ navigations = [x \in {} |-> 0]
    /\ freeNavIDs  = NavIDs
    /\ freeNkIDs   = NkIDs
    /\ navigationStartQueue = <<>>

CreateNavigable ==
    /\ freeNavIDs # {}
    /\ LET nID == CHOOSE x \in freeNavIDs : TRUE
       IN
       /\ navigables' = navigables @@
              (nID :> [ parent               |-> NONE
                      , doc_status           |-> "loaded"
                      , before_unload_status |-> {}
                      , nav_status           |-> "InitiallyFinalized"
                      , ongoing_nav_id       |-> NONE ])
       /\ freeNavIDs' = freeNavIDs \ {nID}
    /\ UNCHANGED <<navigations, freeNkIDs, navigationStartQueue>>

CreateChildNavigable(parentID) ==
    /\ freeNavIDs # {}
    /\ LET nID == CHOOSE x \in freeNavIDs : TRUE
       IN
       /\ navigables' = navigables @@
              (nID :> [ parent               |-> parentID
                      , doc_status           |-> "loaded"
                      , before_unload_status |-> {}
                      , nav_status           |-> "InitiallyFinalized"
                      , ongoing_nav_id       |-> NONE ])
       /\ freeNavIDs' = freeNavIDs \ {nID}
    /\ UNCHANGED <<navigations, freeNkIDs, navigationStartQueue>>

CreateNavigation(navableID) ==
    /\ freeNkIDs # {}
    /\ LET nk == CHOOSE x \in freeNkIDs : TRUE
       IN
       /\ navigations' = navigations @@
              (nk :> [ navigable |-> navableID
                      , status    |-> "started"
                      , affected  |-> {} ])
       /\ freeNkIDs' = freeNkIDs \ {nk}
       /\ navigationStartQueue' = Append(navigationStartQueue, nk)
    /\ UNCHANGED <<navigables, freeNavIDs>>

StartNavigating(nk) ==
    /\ LET nav       == navigations[nk]
           navableID == nav.navigable
           affected  == {navableID} \cup Descendants(navableID)
       IN
       /\ nav.status = "started"
       /\ Len(navigationStartQueue) > 0
       /\ Head(navigationStartQueue) = nk
       /\ navigations' = [navigations EXCEPT ![nk].affected = affected]
       /\ navigables' =
              [n \in ActiveNavigables |->
                  IF n \in affected
                  THEN [ navigables[n] EXCEPT
                         !.before_unload_status =
                             @ \cup {<<"Queued", nk>>},
                         !.nav_status     = "WaitingForUnload",
                         !.ongoing_nav_id =
                             IF n = navableID THEN nk
                             ELSE navigables[n].ongoing_nav_id ]
                  ELSE navigables[n]]
    /\ navigationStartQueue' = Tail(navigationStartQueue)
    /\ UNCHANGED <<freeNavIDs, freeNkIDs>>

RunBeforeUnload(navableID, nk, outcome) ==
    /\ <<"Queued", nk>> \in navigables[navableID].before_unload_status
    /\ navigables' =
           [navigables EXCEPT
               ![navableID].before_unload_status =
                   (@ \ {<<"Queued", nk>>}) \cup {<<outcome, nk>>}]
    /\ UNCHANGED <<navigations, freeNavIDs, freeNkIDs, navigationStartQueue>>

AllResolved(navableSet, nk) ==
    \A n \in navableSet :
        \/ <<"Approved", nk>> \in navigables[n].before_unload_status
        \/ <<"Aborted",  nk>> \in navigables[n].before_unload_status

AnyAborted(navableSet, nk) ==
    \E n \in navableSet :
        <<"Aborted", nk>> \in navigables[n].before_unload_status

ContinueNavigation(nk) ==
    /\ LET nav       == navigations[nk]
           navableID == nav.navigable
           affected  == nav.affected
       IN
       /\ nav.status = "started"
       /\ affected # {}
       /\ AllResolved(affected, nk)
       /\ LET newStatus ==
                  IF AnyAborted(affected, nk) THEN "aborted"
                  ELSE "finalized"
          IN
          /\ navigations' =
                 [navigations EXCEPT ![nk].status = newStatus]
          /\ navigables'  =
                 [n \in ActiveNavigables |->
                     IF n \in affected
                     THEN [ navigables[n] EXCEPT
                            !.nav_status     = "Finalized",
                            !.ongoing_nav_id =
                                IF n = navableID THEN NONE
                                ELSE navigables[n].ongoing_nav_id ]
                     ELSE navigables[n]]
    /\ UNCHANGED <<freeNavIDs, freeNkIDs, navigationStartQueue>>

Done ==
    /\ \A nk \in ActiveNavigations :
           navigations[nk].status \in {"finalized", "aborted"}
    /\ UNCHANGED vars

Next ==
    \/ CreateNavigable
    \/ \E parentID  \in ActiveNavigables              : CreateChildNavigable(parentID)
    \/ \E navableID \in ActiveNavigables              : CreateNavigation(navableID)
    \/ \E nk        \in ActiveNavigations             : StartNavigating(nk)
    \/ \E navableID \in ActiveNavigables,
          nk        \in NkIDs,
          outcome   \in {"Approved", "Aborted"}       : RunBeforeUnload(navableID, nk, outcome)
    \/ \E nk        \in ActiveNavigations             : ContinueNavigation(nk)
    \/ Done

Spec == Init /\ [][Next]_vars

FinalizedImpliesAllApproved ==
    \A nk \in ActiveNavigations :
        navigations[nk].status = "finalized" =>
            LET affected      == navigations[nk].affected
                participating == {n \in affected :
                                    \E s \in BeforeUnloadState :
                                        <<s, nk>> \in navigables[n].before_unload_status}
            IN
            \A n \in participating :
                <<"Approved", nk>> \in navigables[n].before_unload_status

=============================================================================