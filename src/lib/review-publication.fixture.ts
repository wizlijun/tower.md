import type {PreparedReview} from './review';

// Exact serde_json baseline.approved and baseline.created data from temporary agent_qa B0.
// Source: artifacts/app-524fd29-actual-publication-events-01.json; not P7 acceptance.
export const serialized={
  "approval": {
    "approval": {
      "actor": "human:agent_qa",
      "approval_eid": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
      "binding": {
        "content_hash": "4efb77695c63a04eac0cb4ea39cf874d9fa8ea95e968c3c37b203125563be4ae",
        "draft_revision": "3a496f46d0c607cd86527b3b53423145ddd12f33",
        "object_id": "01a154cb-af81-70f3-8598-96c6be435444",
        "project_id": "01a154cb-a8b3-744f-91d1-4992b6a028ca",
        "target_baseline": "towermd/B0"
      },
      "comment": "agent_qa mechanical B0 fixture only; not P7 human acceptance",
      "verdict": "approved"
    },
    "p3_receipt": {
      "body_hash": "cc18f92bd0eb91a033efb1a9b6e1851e9a61e35184522b29ce8700d1614b10f6",
      "request_id": "01a154cb-e999-7897-b215-7e4a0fa0b345",
      "result_ref": {
        "actor": "human:agent_qa",
        "approval_eid": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
        "binding": {
          "content_hash": "4efb77695c63a04eac0cb4ea39cf874d9fa8ea95e968c3c37b203125563be4ae",
          "draft_revision": "3a496f46d0c607cd86527b3b53423145ddd12f33",
          "object_id": "01a154cb-af81-70f3-8598-96c6be435444",
          "project_id": "01a154cb-a8b3-744f-91d1-4992b6a028ca",
          "target_baseline": "towermd/B0"
        },
        "comment": "agent_qa mechanical B0 fixture only; not P7 human acceptance",
        "verdict": "approved"
      }
    },
    "p3_request": {
      "body_hash": "cc18f92bd0eb91a033efb1a9b6e1851e9a61e35184522b29ce8700d1614b10f6",
      "request_id": "01a154cb-e999-7897-b215-7e4a0fa0b345"
    },
    "request_body": {
      "action": {
        "binding": {
          "content_hash": "4efb77695c63a04eac0cb4ea39cf874d9fa8ea95e968c3c37b203125563be4ae",
          "draft_revision": "3a496f46d0c607cd86527b3b53423145ddd12f33",
          "object_id": "01a154cb-af81-70f3-8598-96c6be435444",
          "project_id": "01a154cb-a8b3-744f-91d1-4992b6a028ca",
          "target_baseline": "towermd/B0"
        },
        "comment": "agent_qa mechanical B0 fixture only; not P7 human acceptance",
        "kind": "review_baseline",
        "verdict": "approved"
      },
      "actor": "human:agent_qa",
      "host": "01a154cb-a8e4-77bd-9b1f-70dd6026245f",
      "project_id": "01a154cb-a8b3-744f-91d1-4992b6a028ca",
      "run": null
    }
  },
  "created": {
    "approval_eid": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
    "commit": "c46104384cec8aa4c78446b50b4250b2d966db14",
    "content_hash": "4efb77695c63a04eac0cb4ea39cf874d9fa8ea95e968c3c37b203125563be4ae",
    "p3_receipt": {
      "body_hash": "234a87d48c67bd135c0f370e22ba891207a92d0bb45233c15f9ecd67fea7d2a4",
      "request_id": "git:89fe8afcb8277b79759988818fe3c0e26a2bca4da3bfb06c7618fac9b2d84fff",
      "result_ref": {
        "approval_eid": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
        "commit": "c46104384cec8aa4c78446b50b4250b2d966db14",
        "content_hash": "4efb77695c63a04eac0cb4ea39cf874d9fa8ea95e968c3c37b203125563be4ae",
        "publication_id": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
        "tag": "towermd/B0",
        "tags": [
          "towermd/B0"
        ],
        "target": "towermd/B0"
      }
    },
    "p3_request": {
      "body_hash": "234a87d48c67bd135c0f370e22ba891207a92d0bb45233c15f9ecd67fea7d2a4",
      "request_id": "git:89fe8afcb8277b79759988818fe3c0e26a2bca4da3bfb06c7618fac9b2d84fff"
    },
    "publication_id": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
    "request_body": {
      "action": {
        "kind": "record_baseline",
        "result": {
          "approval_eid": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
          "commit": "c46104384cec8aa4c78446b50b4250b2d966db14",
          "content_hash": "4efb77695c63a04eac0cb4ea39cf874d9fa8ea95e968c3c37b203125563be4ae",
          "publication_id": "git:cf2653f1c41eaaa237d35de5be9681da1d4daac665e90a99b32513732723aec3",
          "tag": "towermd/B0",
          "tags": [
            "towermd/B0"
          ],
          "target": "towermd/B0"
        }
      },
      "actor": "process:towermd-baseline",
      "host": "01a154cb-a8e4-77bd-9b1f-70dd6026245f",
      "project_id": "01a154cb-a8b3-744f-91d1-4992b6a028ca",
      "run": null
    },
    "tag": "towermd/B0",
    "tags": [
      "towermd/B0"
    ],
    "target": "towermd/B0"
  }
};

// Preserve the serialized wire shape when adapting the subject for component tests.
export function publicationFixture(original:PreparedReview,approval:string) {
 const saved=structuredClone(serialized.created);
 const result={commit:saved.commit,publication_id:approval,approval_eid:approval,content_hash:original.binding.content_hash,target:original.binding.target_baseline,tag:original.binding.target_baseline,tags:[original.binding.target_baseline]};
 return {...result,request_body:{...saved.request_body,project_id:original.binding.project_id,run:null,action:{kind:'record_baseline',result:structuredClone(result)}},p3_request:saved.p3_request,p3_receipt:{...saved.p3_receipt,result_ref:structuredClone(result)}};
}
